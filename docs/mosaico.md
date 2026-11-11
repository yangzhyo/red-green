# ESP-Mosaico 开发与验证

对应 #18，CoreBoard V1.2.1，USB-C。此文随实施更新；没有完成真机验收前，不将构建成功写成硬件适配完成。

## 固定依赖

- ESP-IDF 6.2-dev：`0e6b480f858c69862ffb6001d96e9eef74599a6e`，递归初始化官方子模块。
- 官方 BSP：`716fc7bca30ed9c61166a0343bf6f5f995b5131f`，包含 BSP 相邻的 SPI NAND 组件。
- TinyUSB / LVGL 及传递依赖由 `firmware/mosaico/dependencies.lock` 固定。
- cJSON v1.7.19 在仓库保留原始 MIT 许可；主机测试与设备共用解析实现。

首次拉取执行 `git submodule update --init --recursive` 获取固定 BSP。在锁定 SDK 目录运行 `./install.sh esp32s31`，再设置 `RED_GREEN_IDF_PATH`、`IDF_TOOLS_PATH`：

```sh
bash scripts/mosaico-env.sh --preview set-target esp32s31
bash scripts/mosaico-env.sh build
sh firmware/mosaico/tests/run.sh
```

`sdkconfig.defaults` 中的串口是业务 CDC，日志使用 UART；不得将业务 CDC 同时用于 console 或开发工具。桌面自动发现 Espressif VID 下产品名为 `red-green Mosaico` 的设备，并进一步校验应用、资源、硬件与启动代次。

`RED_GREEN_MOSAICO_PORT` 仅用于显式指定测试 PTY 或诊断端口，正常运行无需设置。

## 资源与真实界面预览

三套精灵、六种状态的全部 26 帧和 CSS 动作由 `node scripts/gen-mosaico-assets.mjs` 导出；12 段现有 WAV 离线转成 48 kHz / PCM16 / 双声道。字体、源资产和转换结果共同参与 SHA256 握手，资源不匹配不接管。

固定气泡采用 32 px 预置彩色图片：💤、🚩❗、💥、👋💬、✨，运行中仍显示动词。图形来自 Twemoji（CC BY 4.0），[来源、许可与重建说明](../firmware/mosaico/assets/twemoji/README.md)随仓库保留。先在 `app/` 执行 `pnpm install --frozen-lockfile`，再导出资源；不依赖设备的 emoji 字体。更新图片资源后必须配套重建桌面应用和固件。

字体使用 Noto Sans CJK SC（SIL OFL，许可保留在生成目录），固定文案另有 16 px 子集；20/28 px、1 bit，覆盖字体中 U+0020–FFFF 的可用字形。执行 `cd app && pnpm install --frozen-lockfile` 后，`bash scripts/gen-mosaico-fonts.sh` 可从校验 SHA256 的固定上游提交重建。**超出字集的项目名会拒绝设备快照并回退电脑**，不显示方框。字体约 5.3 MiB，含彩色气泡的完整应用约 6.97 MiB；实物应用槽位为 7 MiB，构建使用已核对的布局检查容量。

原生预览运行与固件相同的 LVGL 界面代码，支持检查中文长名称和五行总览；不模拟物理屏幕、codec 或 USB：

```sh
cmake -S firmware/mosaico/tests/preview -B /tmp/rg-preview -G Ninja
cmake --build /tmp/rg-preview -j 8
/tmp/rg-preview/preview firmware/mosaico/tests/preview/five.json /tmp/rg-five.ppm 2000
python3 firmware/mosaico/tests/preview/bubbles.py /tmp/rg-preview/preview
python3 firmware/mosaico/tests/preview/metro.py /tmp/rg-preview/preview
ctest --test-dir /tmp/rg-preview --output-on-failure
```

需先通过固件配置取得锁定的 LVGL 依赖。`sh firmware/mosaico/tests/run.sh` 在主机使用 ASan/UBSan 验证授权期限、去重、过期、帧上限与轮播公平性。`node --test app/tests/*.test.js` 验证资源可重跑及设备用量投影。

气泡回归在真实 LVGL 中切换六种状态，检查三套皮肤下的颜色、可辨认尺寸、状态切换与运行中文字；[渲染预览](images/mosaico-bubbles-lvgl.png)。这次修复保留原宠物动效，已更新到 CoreBoard V1.2.1 与电脑常驻应用，用户确认符号清楚、颜色正常且没有遮挡。

2026-11-11 气泡修复实机验收：写入前备份当前应用与前方保护区域，并与设备摘要核对；仅更新 `main_app`，写后应用校验通过，前方启动器/分区表/Vibe/NVS 与后方资源区摘要均未变化。新资源版本为 `d2db1f1e250060c389545f36886699c9d563e80570fe2e7cc33cacb32ff641e3`，设备显示与音频健康通过。六状态每项显示 6 秒、连续两轮，12 个快照版本和 148 次健康续期通过；用户确认五组图形与运行中文字显示正常。检查结束后已恢复 LaunchAgent、真实会话和账号用量，设备重新接管；原宠物动效保持不变。

设备授权到期或显示/音频无进展时，高优先级任务关闭功放并切回等待画面；显示锁无法取得则拉低屏幕复位并重启。该软件路径已编译和模拟验证，硬故障下物理屏幕/声音的实际截止时间待真机测量。

Claude 的 `updated_at` 表示数值最后变化时间，设备显示“数值更新于 …”；没有可信 `resets_at` 时显示未知，到重置时刻后标为过期。Codex 继续使用成功快照 120 秒时效。

## Metro 界面与空闲熄屏（#22）

新版沿用“主角＋总览”黄金分栏，底色为纯黑，用量改为两条进度条，右侧保留五行固定色块。主角色块的背景和文字一起按 3 秒周期变化：显示 2 秒、淡出 0.4 秒、全隐 0.2 秒、淡入 0.4 秒；其它行保持显示。顶底文字内收 65 px，按保守 R96、内缩 8 px 的轮廓检查；这不是厂商标注的圆角半径。原宠物动作和彩色气泡保持不变；按用户实机反馈，气泡容器透明，取消白色底板与尾巴，运行中文字使用浅色。

用户已确认：所有会话连续 idle 30 秒后关闭面板，触摸可亮屏至最后一次触摸后 30 秒。持续按住会续时；重复空闲快照和用量更新不续时。所有分页参与判断，任一非 idle（含待确认、轮到你、已完成、异常）自动唤醒并保持亮屏，无会话也视为空闲。

只关闭 CO5300 的 Display On/Off，不执行 Sleep In 或 MCU 深睡。暂停显示刷新并禁止滚动标签请求新帧，USB、授权、触摸轮询继续工作，熄屏期间仍由设备接管；恢复时先提交最新完整帧，再点亮面板。失联后触摸只显示等待连接，不显示旧会话。亮屏健康需要实际画面提交，另在官方 panel draw_bitmap 接口锁存底层写屏错误，避免 adapter 的空完成事件掩盖失败；熄屏健康需要关闭面板成功且 UI 计时器继续进展，面板控制失败仍走既有故障回退。

原生 LVGL 已覆盖计时、重复快照、长按/重复触摸、跨页活动、授权撤销、无会话、面板开关失败、没有实际重绘的健康超时。用户已确认新 Metro 布局及圆角文字正常、全空闲约 30 秒熄屏、触摸唤醒且 30 秒后再次熄屏、第六个运行会话自动唤醒并保持显示，以及熄屏失去授权后触摸只见等待连接；检查期间 750 次健康续期、4 个快照版本通过且设备启动代次未变化。透明气泡修正已通过真实 LVGL 回归并写入最终固件；用户确认白底与尾巴已消失，符号/文字清楚、其他界面正常。常驻应用已恢复真实会话和用量。

本次最终固件为 `23ea7a0`，7,310,176 / 7,340,032 字节，SHA256 为 `0de499b8218405553bc242f1ad99776659317239ea9a5c054ea9d984492dedf2`；配套资源版本为 `cc283b9c09fd209d75ede06b6d4fb2ec0f4725ccccded7635b3fd98e4145a5cf`。两次更新均先备份当前应用及保护区域，写后应用摘要通过，前后保护区域保持不变。两小时连续运行仍按用户安排稍后执行。

[最终 Metro 原生渲染预览](images/mosaico-metro-lvgl.png)。

## 本地 Idea 保存与切换（#23）

2026-11-11 已在这台 CoreBoard V1.2.1、Vibe Mode 0.3.0 上完成「保存当前开发版」及从保存条目重新启动的实机验证。无需上传社区或等待审核。实际入口如下：

1. 保持 USB 连接，单击 POWER 关机；按住 **AI** 键、单击 POWER 开机，看到 Vibe Mode 后松开 AI。此时电脑自动恢复宠物与用量表。
2. 进入「开发 → 保存当前开发版」，确认名称后点键盘 GO，等待完成；本机命名输入仅支持 ASCII、最多 32 个字符。
3. 在「我的 IDEA」选择保存条目，点「运行」，保持供电直到设备重新启动。red-green 与电脑完成应用、资源及显示/音频健康握手后，设备重新接管。

本次已保存的条目：

| 项目 | 实测值 |
| --- | --- |
| 名称 | `red_green_mosaico` |
| 本地编号 | `dcd1bf`（不是社区六位下载码） |
| 版本 | `23ea7a0` |
| 设备显示大小 | 14,249 KiB，约 13.9 MiB |
| 保存前存储 | 已用 8 / 114 MiB |
| 原有条目 | `望潮 Tide Watcher`，编号 `688018`、版本 `6.18.0`，保存后仍在目录中 |

Vibe 保存了当前应用、分区表和原布局中的 model/font/product_assets 资源分区，因此副本比约 7 MiB 的 red-green 应用本身大。red-green 当前实际使用的宠物、字体、叫声均嵌入应用。此次没有安装其他应用、上传社区、更新 Vibe 或重写分区布局。

验证证据包括保存成功页、保存前后目录、本地条目详情、打开进度、电脑日志恢复 `Preparing → Device`，以及用户确认真实宠物、状态、用量和触摸正常。Vibe 实时身份与已登记设备一致，启动代次为 `6695187665783910469`、`stale=false`。退出后已释放官方 Gateway，正常应用继续管理业务 USB。

应用 SHA256 仍为上节的 `0de499…2dedf2`。按官方保存算法，用本机已校验应用与此前保持不变的分区表/资源备份重算，得到 release 摘要 `cd1bf709657a3463cbce38c58438db05f21aa4bf55c8b9ad1c52056dd46ccb3a`，对应编号 `dcd1bf`，与设备条目一致。这是本机重算与设备条目的一致性检查，**不是独立导出 NAND 后的逐字节校验**。本轮通过的是保存及同版本打开路径；没有通过安装其他应用来测试 NOR 被覆盖后的完整重装路径。

当前应用独立备份、组件摘要、设备截图及原始日志保存在本机 `~/Library/Application Support/red-green/mosaico-backups/20261111-local-idea/`，不提交设备原始数据。后续更新 red-green 固件后，需要再次保存才能保留新版本；旧条目代表此次保存的版本。只进入 Vibe、不运行其他应用时，松开 AI 正常关机再开机也可返回当前应用；切换过其他应用后，应从「我的 IDEA」打开已保存的 red-green。

## 安装边界

`partitions.csv` 只用于编译容量校验，不是覆盖实物的授权。首次安装前读取真实板型、现有分区和闪存大小，保存可恢复镜像，核对应用槽位足够大后才能写入应用。禁止全片擦除、改写 eFuse，或直接运行会覆盖 bootloader/partition table 的默认 `idf.py flash`。

现有 Vibe/Iris 与维护分区必须保留。对下述已核对布局，只写 `main_app`：

```sh
# 设备须先进入 ROM 下载模式；PORT 是该设备重新枚举出的端口。
/tmp/rg-flash-tools/bin/python -m esptool --chip esp32s31 --port "$PORT" \
  --before no-reset --after hard-reset write-flash \
  --flash-mode keep --flash-freq keep --flash-size keep \
  0x210000 firmware/mosaico/build/red_green_mosaico.bin
```

先比对备份的 MAC、分区表、Flash 容量与镜像目标；本命令只适用于已核对的 `0x210000` / 7 MiB 槽位。esptool 写后会校验摘要；首次部署还独立复核了应用摘要及写前/写后 `0..0x210000` 的保护区域。原数据分区不在写入范围。ROM 下载模式进入方法：关机，按住 USB-C 左侧 BOOT 键开机后松开；进入 Vibe 则是按住 AI 键开机，两者不同。

恢复原应用时，从原 `flash.bin` 提取 `0x210000..0x910000` 的 7 MiB 为 `main-app.bin`，核对备份摘要后用上面相同的应用地址写回；不要把完整 `flash.bin` 作为应用镜像，也不要全片擦除。备份还保留原启动器、Vibe 和数据，但恢复应用无需改写这些区域。

运行桌面应用后无需配置端口。只有 red-green 握手、资源版本及显示/音频健康都通过才接管；运行 Vibe 或其他应用时仍由电脑呈现。日志在 `~/Library/Logs/red-green.log`，可查看 Preparing / Device / Fallback / Desktop 切换。设备初始化失败时的握手包含 `boot_phase`、复位原因及剩余堆空间，健康标记保持 false。

### 只读备份

`scripts/mosaico-backup.py` 使用 esptool 5.6 的 S31 支持。调用前从官方 Iris 的实时设备清单核对 MAC 和板型；脚本只接受对应 MAC 的 Espressif 维护接口或 ROM 接口。Vibe/原厂应用可按官方 DTR/RTS 时序临时进入 ROM；官方 stub 仅加载到 RAM。

```sh
python3 -m venv /tmp/rg-flash-tools
/tmp/rg-flash-tools/bin/pip install esptool==5.6.0
/tmp/rg-flash-tools/bin/python scripts/mosaico-backup.py \
  --port /dev/cu.usbmodem设备端口 --expected-mac 设备MAC \
  --output "$HOME/Library/Application Support/red-green/mosaico-backups/唯一日期目录"
```

两次完整读取必须逐字节一致，才保存 `flash.bin`、`partition-table.bin` 和含 SHA256 的 `manifest.json`。目录权限 700、文件 600；备份可能包含原设备凭据，留在本机，不提交仓库。脚本不含擦除或写 Flash/eFuse 命令。

2026-11-11 实物确认：ESP32-S31 v0.0、板型 eFuse 0x0102、16 MiB NOR、约 16 MiB PSRAM，Vibe 0.3.0 / Recovery ABI 2。分区表 SHA256 为 `f893e928b43d0e497fa9fb07ea383303ba60a8f4b03f425519f29cb6e97856c3`，与官方 Iris 实时清单一致。`main_app` 位于 `0x210000`、容量 `0x700000`；`0x910000` 之后是原有 model/font/product_assets。模板中的更大应用槽位不适用于这台设备。

### 主机与设备核心集成验证

```sh
cargo build --manifest-path app/src-tauri/Cargo.toml --example mosaico_probe
python3 scripts/mosaico-simulator.py
python3 scripts/mosaico-simulator.py --serve
```

模拟器使用真实 C 协议核心，通过 PTY 连接真实 Rust 串口循环；`--serve` 输出端口，供隔离 Tauri 实例使用。它验证单向丢包、渲染停止、manager 停止、迟到授权、重复 Call 与回退后的会话清理。CI 执行 C 核心 ASan/UBSan 和 PTY 集成测试；SDK 固件编译、实际显示和物理声音仍需单独验收。

本地已验证：Rust 36 项、前端 13 项、设备核心与调度、上述 PTY 场景、固定资源完整重建、LVGL 原生预览，以及 Tauri 实际窗口/afplay 的设备独占和桌面回退。回退恢复当前会话，设备期间删除的会话不会复活。原生预览不代表物理屏幕验收。

**真机验收进行中：** 启动阶段的 2 秒任务看门狗过早触发，实测复位原因 6，停在显示初始化；已改为初始化完成后监控高优先级业务看门任务，保留运行时 2 秒保护。设备已进入 running，显示/音频健康通过，剩余堆约 2.9 MB。续期只延长授权，不重复切换界面或争用显示锁。用户已确认五会话画面与中文可读性正常、设备单次叫声且电脑静默；实际拔插能回退与重连，没有补播。用户两次睡眠唤醒测试确认通过且没有补播；切到真实会话后，用户确认项目、状态与账号用量一致。两小时连续运行测试由用户明确安排稍后执行，Issue #18 保持打开，PR 保持 Draft。
