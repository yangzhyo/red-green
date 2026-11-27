use super::{document, escaped, pair_form, send, Pairing, Source, HTML};
use crate::reader::server::submit_reply;
use std::{io::Read, net::SocketAddr, sync::Mutex};
use tiny_http::{Method, Request};

fn explanation(status: u16) -> &'static str {
    match status {
        400 => "表单无效，请回到正文重新打开「回复」。",
        403 => "请在 Mac 菜单开启「允许所有已配对设备回复 Codex」，再从正文打开回复。",
        404 | 410 => "会话已结束或无法找到。",
        405 => "此会话只支持阅读。",
        409 => "当前回合正在进行，请等它结束后再发送。如有审批提示，请在 Mac 上处理。",
        412 => "发送凭据已失效。请先查看最新正文，确认是否已经发送。",
        413 | 422 => "请输入 1 至 8192 个字。",
        424 => "Codex 未受理这条回复，请回到正文后重试。",
        502 => "发送结果待核对，请先查看最新正文。",
        _ => "会话暂时无法使用，请稍后重试。",
    }
}

fn composer(path: &str, read: &str, ticket: &str, draft: &str, hint: &str) -> String {
    format!(
        r#"<h1>回复</h1><p>{}</p><form method="post" action="{}"><input type="hidden" name="ticket" value="{}"><label for="text">回复内容</label><textarea id="text" name="text" rows="5" required>
{}</textarea><nav><a href="{}">取消</a> <button type="submit">发送</button></nav></form>"#,
        escaped(hint),
        escaped(path),
        escaped(ticket),
        escaped(draft),
        escaped(read)
    )
}

fn submission(req: &mut Request, address: SocketAddr) -> Result<(String, String), u16> {
    let field = |name: &'static str| {
        req.headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str())
    };
    let origin = format!("http://{address}");
    let same_origin = match field("Origin") {
        Some(value) => value == origin,
        None => field("Referer").is_some_and(|url| url.starts_with(&format!("{origin}/"))),
    };
    if !same_origin
        || field("Content-Type").and_then(|v| v.split(';').next())
            != Some("application/x-www-form-urlencoded")
    {
        return Err(403);
    }
    // 8192 Unicode scalars, up to four UTF-8 bytes and three form bytes per byte.
    const LIMIT: usize = 8192 * 12 + 256;
    if !req.body_length().is_some_and(|n| n <= LIMIT) {
        return Err(413);
    }
    let mut data = Vec::new();
    req.as_reader()
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|_| 400u16)?;
    let (mut ticket, mut text) = (None, None);
    for (key, value) in form_urlencoded::parse(&data) {
        match key.as_ref() {
            "ticket" if ticket.is_none() => ticket = Some(value.into_owned()),
            "text" if text.is_none() => text = Some(value.replace("\r\n", "\n")),
            _ => return Err(400),
        }
    }
    Ok((ticket.ok_or(400u16)?, text.ok_or(400u16)?))
}

pub(super) fn handle(
    mut req: Request,
    address: SocketAddr,
    encoded_key: &str,
    pairing: &Mutex<Pairing>,
    cookie: &str,
    source: &mut impl Source,
) {
    let read = format!("/kindle/read/{encoded_key}");
    let path = format!("/kindle/reply/{encoded_key}");
    let mut draft = String::new();
    let result = (|| -> Result<(u16, String), u16> {
        let key = percent_encoding::percent_decode_str(encoded_key)
            .decode_utf8()
            .map_err(|_| 400u16)?;
        {
            let device = pairing.lock().unwrap();
            if !device.allows(cookie) {
                return Err(401);
            }
            if !key.starts_with("codex-") {
                return Err(405);
            }
            if req.method() == &Method::Get && !device.replies_enabled() {
                return Err(403);
            }
        }
        match *req.method() {
            Method::Get => {
                // Opening a saved composer also selects its live Codex thread.
                let page = source.kindle_messages(&key, "")?;
                let mut device = pairing.lock().unwrap();
                if !device.allows(cookie) {
                    return Err(401);
                }
                let ticket = device.reply_ticket(&key).ok_or(403u16)?;
                Ok((
                    200,
                    composer(
                        &path,
                        &read,
                        ticket,
                        "",
                        page["title"].as_str().unwrap_or("当前会话"),
                    ),
                ))
            }
            Method::Post => {
                let (ticket, text) = submission(&mut req, address)?;
                draft = text;
                if !pairing.lock().unwrap().replies_enabled() {
                    return Err(403);
                }
                match submit_reply(source, pairing, cookie, &key, &ticket, &draft) {
                    Ok(()) => Ok((303, String::new())),
                    Err(code @ (409 | 422 | 503)) => {
                        let device = pairing.lock().unwrap();
                        if !device.allows(cookie) {
                            return Err(401);
                        }
                        device.check_ticket(&key, &ticket)?;
                        Ok((
                            code,
                            composer(&path, &read, &ticket, &draft, explanation(code)),
                        ))
                    }
                    Err(status) => Err(status),
                }
            }
            _ => Err(405),
        }
    })();
    if !pairing.lock().unwrap().allows(cookie) || matches!(result, Err(401)) {
        return pair_form(req, pairing, 401, "请先配对，再打开会话回复。");
    }
    match result {
        Ok((303, _)) => send(
            req,
            303,
            HTML,
            String::new(),
            None,
            Some(&format!("{read}?latest=1")),
        ),
        Ok((status, body)) => send(
            req,
            status,
            HTML,
            document("回复", &body, false),
            None,
            None,
        ),
        Err(status) => {
            let body = format!(
                r#"<h1>回复</h1><p>{}</p><pre>{}</pre><p><a href="{}?latest=1">查看最新正文</a> · <a href="/kindle">返回列表</a></p>"#,
                explanation(status),
                escaped(&draft),
                escaped(&read)
            );
            send(
                req,
                status,
                HTML,
                document("回复", &body, false),
                None,
                None,
            )
        }
    }
}
