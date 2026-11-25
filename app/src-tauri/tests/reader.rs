use red_green::reader::{Library, Reader};
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
};

fn request(server: &Reader, method: &str, path: &str, body: &str, cookie: &str) -> String {
    let mut stream = TcpStream::connect(server.address()).unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\nCookie: {cookie}\r\n\r\n{body}", server.address(), body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn reader_pairing_and_revocation() {
    let reader = Reader::start(
        "127.0.0.1:0".parse().unwrap(),
        Arc::new(Mutex::new(Library::default())),
    )
    .unwrap();
    for path in ["/sessions", "/sessions/demo/messages"] {
        assert!(request(&reader, "GET", path, "", "").starts_with("HTTP/1.1 401"));
    }
    let code = reader.new_code();
    let body = serde_json::json!({"code":code}).to_string();
    let response = request(&reader, "POST", "/pair", &body, "");
    assert!(response.starts_with("HTTP/1.1 200"));
    let cookie = response
        .lines()
        .find_map(|s| s.strip_prefix("Set-Cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert!(request(&reader, "POST", "/pair", &body, "").starts_with("HTTP/1.1 403"));
    assert!(request(&reader, "GET", "/sessions", "", cookie).starts_with("HTTP/1.1 200"));
    reader.revoke();
    assert!(request(&reader, "GET", "/sessions", "", cookie).starts_with("HTTP/1.1 401"));
}
