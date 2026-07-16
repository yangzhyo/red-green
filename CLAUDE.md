# red-green 开发纪律

沟通、commit、issue、PR 一律中文。领域词汇以 CONTEXT.md 为准，新词先进词汇表再用。

## Loop：issue 驱动的开发闭环

**入队** — 非琐碎的活（改行为、改设计）必须先有 issue 再动工；用户口述想法时当场代写，不打断节奏。issue 必含两段：**动机**（为什么做）与**验收标准**（做到什么算完——可验证的行为描述，用领域词汇写，兼作 /verify 剧本）。不用 issue 模板。

**琐碎豁免** — typo、注释、纯文案级修补：不建 issue、不开 PR，直推 main。

**领取** — 用户说「做 #N」：读 issue → 开分支（issue-N-英文短语）→ 实现 → 本地 /verify 验证。实现全部在本地会话完成，不派云端 agent。

**交付** — 走 issue 的必走 PR，描述含 `Closes #N`。CI 必须绿（检查项以 .github/workflows/ci.yml 为准）。

**审查** — PR 开出后跑 /code-review 双轴审：Standards（合不合仓库规范）、Spec（对不对得上 issue 验收标准）。评审毕**必在 PR 留一条小结评论，无论有无发现**——有发现逐条贴；无发现也写明双轴通过并点出 Spec 对齐了哪几项验收标准（如「Standards 无发现；Spec 对齐 #N 全部验收项」）。没有这条小结＝审查未完成、不予合并——它既是审查步的完成定义，也是用户终审的可见抓手。

**合并** — 只在用户说「合」之后执行：`gh pr merge --squash --delete-branch`。squash commit 信息沿用「type: 中文描述」风格；main 保持线性史。

**队列** — 待办唯一真相源是 issue 队列；不在记忆或其他文档里另立待办清单。

## 运行与验证

- 宠物以 .app 常驻（LaunchAgent `dev.y9g.red-green`）；开发迭代前先 `launchctl unload` 该 agent，再在 app/ 下 `pnpm tauri dev`。
- 端到端验证用 /verify skill：注入假会话状态文件，观察叫声（afplay 进程参数）与宠物窗口渲染。
