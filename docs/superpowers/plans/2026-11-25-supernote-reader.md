# Supernote Nomad 会话阅读实施计划

按 `superpowers:executing-plans` 在本地逐项实施。勾选框表示已经有对应的代码或实测结果。

**目标**　在 Supernote Nomad 上选取 Mac 的会话，回看对话，并自动读到新消息。[已确认的设计](../specs/2026-11-25-supernote-reader-design.md) · [需求 #27](https://github.com/yangzhyo/red-green/issues/27)

**架构与技术**　Tauri 的 Rust 进程提供独立的只读 HTTP 服务，复用已有会话发现逻辑。先用合成正文跑通相同的页面、配对和分页协议，再接 Claude Code 记录及 Codex App Server。Nomad 使用原生 HTML、CSS、JavaScript，无需安装应用。

## 全局约束

- 首版服务于同一可信局域网内的 Mac 与 Nomad（A6 X2）。共享默认关闭。未配对设备打开固定链接只能看到配对入口。
- 来源限定为 red-green 当前识别且可以取到正文的 Claude Code、Codex CLI 会话。仅展示用户和助手面向读者的文字。
- 页面在前台每 3 秒检查更新。正常局域网内，从来源提供正文到屏幕可见的初始目标为 5 秒。实际延迟以 Nomad 实测为准。
- 阅读请求独立于桌面状态轮询。保留现有聚焦、已阅、叫声及输出行为。设备离线且 CLI 退出时，仅保留阅读页已加载的正文。

## 审查重点

| 场景 | 预期及负责步骤 |
| --- | --- |
| 已配对设备被撤销时仍在轮询 | 下一请求失去列表及正文访问权，见步骤一 |
| 两个同名项目、来源间 ID 相同或旧页请求晚返回 | 阅读页始终显示所选会话，见步骤二 |
| 向前补页时长回复仍在变长 | 消息不重复，首个可见段落不跳动，见步骤二 |
| Claude 连续运行事件、记录末行只写了一半 | 补齐映射，等完整记录后再发布，见步骤三 |
| Codex 暂时不可用、重连或来源确认已结束 | 仅明确结束才提示结束，重连补齐并去重，见步骤四、五 |

## 一、用样本会话打通固定入口和配对

文件：新增 `app/src-tauri/src/reader/{mod,server,pairing}.rs`、`app/src-tauri/examples/nomad_reader_probe.rs` 与 `app/ui/reader.html`，在 `app/src-tauri/src/lib.rs` 导出 reader。需要 HTTP 或随机数依赖时同步更新 Cargo.toml 和 Cargo.lock。

- [x] 为配对编写回环地址集成测试。匿名请求列表和正文均返回 401；一次性短码成功一次后失效；凭据可跨页面重开使用；撤销后同一凭据立即返回 401。用 `cargo test --manifest-path app/src-tauri/Cargo.toml --lib reader::` 先确认用例失败。
- [x] 落实页面与只读数据边界：`POST /pair` 交换设备凭据，`GET /sessions` 返回会话目录，`GET /sessions/{key}/messages` 返回正文。凭据不进入 URL；短码有有效期和尝试次数限制。读接口以服务端目录解析 key。HTTP 仅提供阅读所需的静态资源和路由。
- [ ] 样本程序绑定显式指定的局域网地址，打印入口和一次性码，从标准输入追加合成对话。运行上述测试应全绿。执行 `cargo run --manifest-path app/src-tauri/Cargo.toml --example nomad_reader_probe -- --bind <Mac局域网地址>:8769`。在 Nomad 笔记保存链接，验证可打开、配对、重开，记录主机名与 IP 各自结果。

## 二、用同一样本验证跟随、回看和休眠恢复

文件：新增 `app/src-tauri/src/reader/model.rs`、`app/ui/reader.js`、`app/ui/reader.css`、`app/tests/reader.test.js`，扩充步骤一的程序和页面。

- [x] 固定来源与网页共用的数据约定。目录项含 key、项目名、来源和简短标识；消息含稳定 ID、角色、顺序和正文。首次获取最近一页；`before` 取更早页，`since` 获取之后新增或修订的消息。响应分别给出更早页游标与更新游标；游标只对原会话有效。过期时返回重新定位结果，按消息 ID 合并已显示的内容。
- [x] 编写页面状态测试：零变一自动进入、多个时展示列表、主动返回后留在列表、切会话后丢弃旧响应、回复增长时仍只有一条、重复响应不产生 DOM 更新、回看并补载前页后保持同一锚点。用 `node --test app/tests/reader.test.js` 先确认失败。
- [x] 实现白底黑字单栏、角色标签、保留换行和缩进的文本渲染；提供上一屏、下一屏和「有新消息」。按消息、段落及屏内偏移还原阅读位置。前台每 3 秒至多一个在途请求；唤醒立即检查；同一轮取数有变化才更新对应节点。测试应全绿。
- [ ] **真机可行性检查**：样本包含两轮中文、跨屏代码和一条分段增长的回复。在正文末尾观察追加，翻回前文后追加两轮，再断网、休眠、唤醒。记录固件版本、位置是否保持、来源时间与屏幕可见时间，以及静置一分钟有无重绘。若固定链接、前台脚本或凭据持久性失败，优先在同一网页方案修正并复测。确认样本能自动刷新后再接真实来源。

## 三、接入 Claude Code 正文

文件：修改 `hooks/session-status.sh`，新增 `app/src-tauri/src/reader/claude.rs` 与 `scripts/tests/test_claude_reader_hook.py`。

- [x] 用临时 HOME、合成钩子事件及合成记录验证：旧状态文件缺映射时，连续「运行中」事件仍可补齐；退出事件使会话离开目录。用 `python3 -m unittest discover -s scripts/tests -p 'test_claude_reader_hook.py'` 先观察失败。
- [x] 在钩子去重提前返回之前采集来源给出的记录位置，位置只保存在本机。保留原状态快照兼容性。重复、并发事件更新映射时使用原子替换。
- [x] 阅读器仅为目录中的会话打开已登记的记录。基于受控记录验证用户消息、助手可见段落、纯工具回合、分支恢复及截断的末行；过滤系统与工具内容。首读与增量读取使用相同的消息标识和顺序，文件替换后重建游标。
- [x] 重跑钩子测试、`bash -n hooks/session-status.sh` 及 Rust reader 测试。用同一会话中的长回复核对末段，确认展示内容超过钩子摘要的 300 字限制。

## 四、接入 Codex CLI 正文

文件：新增 `app/src-tauri/src/reader/codex.rs`，复用 `app/src-tauri/src/codex/rpc.rs` 和现有观察器发布的本机会话清单。

- [ ] 先用专建的样本会话确认本机 App Server 版本、只读分页请求和返回字段；记录进行中与完成后正文何时可见。将合成响应固化为 Rust 单元测试，包括多页逆序、同一回复增长、空回合和含工具输出的回合。
- [ ] 在独立连接上按需取所选线程的正文，映射到步骤二的消息和游标。请求前校验该线程仍属于当前来源及可读目录。以已提供的整条消息或增量更新页面。断开、短暂无数据或目录短暂缺项时保留当前页面并重试。确认来源身份改变时丢弃旧游标和服务端缓存。
- [ ] 用模拟 RPC 断开并恢复，断言恢复后同一回复只出现一次；用同项目两个线程检查隔离。执行 `cargo test --manifest-path app/src-tauri/Cargo.toml --lib`，新用例和已有观察器测试均应通过。

## 五、接入常驻 Mac 应用并验收

文件：修改 `app/src-tauri/src/main.rs`、`app/src-tauri/Cargo.toml`、`README.md` 和 `.github/workflows/ci.yml`。新增 `app/src-tauri/src/reader/control.rs` 管理共享生命周期。

- [ ] 在 Mac 常驻应用增加菜单栏阅读入口，保证无会话时也能操作。提供开启、查看地址与配对码、解除配对、关闭共享；开启前说明可见会话范围。关闭时停止监听并释放端口，再次开启可重新使用已保存的固定地址。用服务生命周期测试覆盖端口占用、重试和撤销。
- [x] 接入两种来源。将来源暂不可用、网络断开与明确结束分别映射为设计稿中的状态；若阅读设备离线时 CLI 退出，只保留已加载的文字。把新增钩子测试纳入 CI，README 补上 Mac 开关、Nomad 笔记链接和地址改变时的操作。
- [ ] 运行项目 CI 中的 cargo check、Rust 单测、Node 测试和现有设备集成检查，并运行新增钩子测试。在 Nomad 按设计稿六项验收逐项记录：固件与 CLI 版本、样本、实测延迟和结果。另在桌面观察阅读、切会话及重连期间的宠物状态、聚焦和叫声。
- [ ] 各步骤通过对应检查后分项提交。最终按项目流程提交关联 #27 的 PR，附自动化与真机结果，并标明尚未验证的项目。


## 实施记录

- 样本程序支持从标准输入追加文字，也会每 12 秒生成或续写一条合成回复。配对码十分钟有效，输入 `/code` 可换码，`/quit` 停止服务。配对回环测试的实际命令为 `cargo test --manifest-path app/src-tauri/Cargo.toml --test reader`。
- 在桌面 Chromium 用合成正文实测了配对、自动追加、主动返回最新、页面重开、断网重连，以及新消息到达时保留回看位置。用户已在 Nomad 上确认样本验证成功。未取得逐项延迟和固件版本记录。
- 已在本机 Codex CLI 0.172.0 生成的协议中核对 `thread/turns/list` 的 `itemsView: full`、排序及游标字段。受控样本分别验证 Claude 主分支、部分写入和工具内容过滤，以及 Codex 用户和助手正文投影。

- 常驻应用提供「读」菜单。读取器由 Mac 显式开启，按所选会话读取 Claude 的完整 JSONL 记录或 Codex 完整回合。集成测试覆盖实际磁盘增量、结束事件、模拟 App Server 的补页与续写。真实 CLI 与 Nomad 的端到端延迟尚待单独测量。
