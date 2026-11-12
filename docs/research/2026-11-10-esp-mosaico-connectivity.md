# ESP-Mosaico 连接电脑调研

调研日期：2026-11-10。范围为 ESP-Mosaico CoreBoard V1.2 / V1.0 与 ESP32-S31；未接真机、未编译或刷写固件。下文将硬件、出厂应用、可开发能力分别说明；方案建议尚未经用户决定。

## 结论

ESP-Mosaico 可以通过 **USB-C 或局域网 Wi-Fi** 与电脑通信。USB-C 接的是 ESP32-S31 的原生 USB 2.0 High-Speed OTG，并非独立 USB-UART 桥；能否在运行时作为 CDC 虚拟串口使用，取决于正在运行的固件。官方已有 CDC、ESP-Iris USB/TCP 通信及设备端图形、音频开发基础，但没有证据表明任意出厂版本已经实现 red-green 的宠物状态与用量协议。[V1.2 指南][board12]、[V1.0 指南][board10]、[ESP-Iris 连接与身份][gateway]

**建议第一阶段优先 USB 数据连接，同时供电；设备预置动画与叫声，电脑只发送状态、用量与发声事件。** Wi-Fi 可作为后续免数据线方案。建议先验证官方 ESP-Iris 能否承载所需应用消息，再决定复用它或采用精简自定义通道；不必先承诺一个完整框架，也不必假定只能从零编写 USB 栈。这是结合当前需求的工程判断，不是官方产品承诺或用户已选方案。

## 1. USB：两个主板版本的能力及限制

| 接口 | CoreBoard V1.2 | CoreBoard V1.0 | 对电脑通信的意义 |
| --- | --- | --- | --- |
| 板载 USB-C | 原生 USB 2.0 HS OTG；供电、充电、下载、应用通信 | 同左 | 推荐直接接电脑的数据接口；须使用数据线，纯充电线不行 |
| 左侧 H2 | Pin 13 = GPIO33 / USJ_DN；Pin 15 = GPIO34 / USJ_DP | 同左 | 另外引出的 USB Serial/JTAG 信号，并非板载 USB-C 的同一通路 |
| 右侧 H1 | Pin 13 = GPIO59 / RX0；Pin 15 = GPIO58 / TX0 | 同左 | UART 调试/下载；接电脑需合适的 USB-UART 转接或调试硬件，不能把 UART 引脚直接当 USB |
| 背部触点 | GND、BOOT、RST、RX、TX、5V、GND | 调试测试点与背扩供电/I2C 分开 | 是辅助开发接口；两个版本的触点定义不应混用 |

以上引脚与连接方式来自两版官方主板指南。两版显示、I2C、音频电源等管脚有变化，固件必须选对板型；“USB 相同”不意味着所有外设配置相同。[V1.2 接口与版本][board12]、[V1.0 接口][board10]

- **硬件/ROM 层：** 板载 HS OTG 在 ROM 下载模式可烧录；芯片本身不会自动让该口成为应用日志串口。USB Serial/JTAG 是另一个固定功能硬件控制器，只能承担串口及 JTAG，不能任意改成显示或音频 USB 类。[主板下载说明][board12]、[ESP-IDF Serial/JTAG 原始文档][usj]
- **出厂固件层：** 主板指南明确出厂固件集成 CDC，提供日志及自动下载。首次烧录或固件不可用时可能需关机、按住 BOOT 再开机；复位会导致 USB 短暂断开。`idf.py monitor` 可能复位设备，调试时可用 `monitor --no-reset`。这些是维护行为，不能当作 red-green 运行时的就绪证明。[主板开发说明][board12]、[官方固件更新][firmware]
- **自定义固件层：** ESP32-S31 的官方状态表已列 TinyUSB USB 2.0 Device 为支持；可以做应用通信，但自定义固件必须保留或实现相应 USB 功能，刷成别的应用后 CDC 不一定存在。官方 TinyUSB 组件目标清单也包含 `esp32s31`。[芯片支持数据][support]、[TinyUSB 组件清单][tinyusb]

USB 日志与业务数据必须明确分离。不能直接把有启动日志、控制台输出和下载控制行为的任意串口当成纯 JSON 流。可评估官方 ESP-Iris 的消息通道；若自行定义 CDC 通道，则需处理帧边界、日志隔离、打开端口时的控制线行为、重枚举与重连。具体采用独立接口还是现有协议封装，尚待选定固件后验证。

已有一个需限定范围的风险证据：**ESP-Brookesia 的 V1.2 适配**只完成部分硬件验证，文档记录其样机自动下载曾断开后不重新枚举，因此默认关闭 `CONFIG_BSP_USB_AUTO_DOWNLOAD`，改为手动进入 ROM；该适配的 TinyUSB CDC 菜单交互可用，但 USB Service 尚未支持此通道上的文件传输。这是该适配的已知范围，不能扩大为“所有 Mosaico USB 不稳定”。[Brookesia V1.2 适配][brookesia]

## 2. Wi-Fi 与蓝牙

| 方式 | 硬件能力 | 出厂应用已公开能力 | 自定义固件与限制 |
| --- | --- | --- | --- |
| Wi-Fi | **2.4 GHz**，802.11b/g/n/ax；不应承诺 5 GHz 或 6 GHz | 可设置 Wi-Fi，并从同局域网电脑通过设备 IP 访问控制面板 | ESP32-S31 文档列出 Station、SoftAP、AP+STA 示例；可在局域网建立应用连接 |
| Bluetooth LE | 主板规格明确 Bluetooth 5.4 LE | 本次未找到出厂应用向电脑开放宠物数据 GATT 服务的官方说明 | 官方当前支持表列 BLE Controller、Bluedroid、NimBLE 为支持；可以开发，但需自定义服务与主机适配 |
| Bluetooth Classic | 主板规格明确 BR/EDR，不是将 ESP32-S3 的能力套用过来 | 本次未找到出厂应用可直接作为电脑 SPP/宠物显示端的说明 | 官方支持表列 Bluedroid Classic 与 SPP 等 profile；不能据此保证现有固件或 macOS 端已经可用 |

硬件依据：[V1.2][board12]、[V1.0][board10]。Wi-Fi 应用依据：[出厂使用指南][factory]、[ESP32-S31 Wi-Fi 示例与模式][wifi]。蓝牙的软件支持依据：[ESP32-S31 官方支持表实际数据][support]，其时间戳为 **2026-11-02T22:17:27.753888Z**。

频段和模式需分开理解：支持 Wi-Fi 6 不代表双频；官方支持表将 802.11ax 限定为 **20 MHz、non-AP**，同时列 Station / SoftAP，所以不能承诺 SoftAP 也是 ax AP。局域网使用建议让设备以 Station 加入有 2.4 GHz 的网络；电脑只需与它 IP 可达，不必强求电脑自身也连 2.4 GHz。后一点是网络架构推论，需要检查具体路由器是否做客户端隔离。[芯片 Wi-Fi 支持状态][support]

蓝牙不是“没有硬件”或“整个 SDK 不支持”；准确结论是：**芯片有，当前 SDK 有相关支持，出厂应用对 red-green 的服务未确认。** 对本项目没有必要用蓝牙传音频流，因为叫声可预置。若用户后续选择蓝牙，应另行实测 macOS 配对/权限、BLE GATT 或 Classic SPP、重连以及与 Wi-Fi 共存；本次没有完成这些验证。[蓝牙 API][bluetooth]、[支持状态][support]

## 3. 扩展模块与其他无线功能

摄像头、多功能交互模块是接到主板的**外设**，不是连接电脑必需的转接器。摄像头通过左侧 H2 的 DVP 与 I2C/SCCB 工作；交互模块提供触摸/按键、PIR、光感、LED、红外等。宠物动画、叫声、用量显示已有板载屏幕与音频资源，无需为电脑通信加这些模块。[摄像头指南][camera]、[交互模块指南][interact]

有两个实际复用限制：摄像头使用 GPIO33/34，与左侧 USB Serial/JTAG 有冲突；板载 USB-C 的 HS OTG 是另一通路。右侧部分引脚与 ES8311 的 I2S 共用，使用扬声器时不能将这些脚再交给扩展用途。[主板引脚说明][board10]、[Brookesia USB 通道说明][brookesia]

官方 BSP 的 `espnow_chat`、磁吸多设备交互与 `mosaico_network` 使用 ESP-NOW 等机制连接多块 Mosaico；这不等于电脑可以直接用普通 TCP/IP 接入该链路。IEEE 802.15.4、Thread/Zigbee 同样属于芯片能力；本次未找到它们作为 Mosaico/macOS 即用连接方式的依据，不纳入首选。[BSP 多设备组件与示例][bsp]

## 4. 官方开发路径与支持状态

| 路径/示例 | 已证实的用途 | 对 red-green 的边界 |
| --- | --- | --- |
| 出厂应用 / 在线更新 | Wi-Fi 设置、设备控制面板、应用安装/维护、浏览器刷写固件 | 不等于已提供本项目的状态/用量接口；需确认设备实际运行版本 |
| ESP-Mosaico BSP | 屏幕、触摸、音频、存储及各外设支持；有 `hmi_demo`、`audio_loopback`、`gmf_audio_player` | 可复用设备呈现能力；预置音频播放已有专门示例 |
| ESP-Mosaico Vibe / ESP-Iris | 官方工作区和 Gateway 有 USB/TCP endpoint、实时 HELLO、Device ID、Boot ID 及连接所有权 | 普通应用须集成并启动 ESP-Iris；可评估复用通信，仍须实现 red-green 的应用语义 |
| `usb_extend_screen` | USB HS 传画面，HID 触摸、UAC 音频；明确针对 Windows 副屏与 IDD 驱动 | **不是 macOS 即插即用副屏方案**；应用占用 TinyUSB/OTG，示例关闭 BSP CDC 并使用 UART 日志 |

逐项来源：[出厂指南][factory]、[官方 BSP][bsp]、[预置 MP3 示例][audio]、[Gateway][gateway]、[运行模式][modes]、[USB 副屏示例][screen]。

正常应用、Vibe Mode、ROM 必须区分：正常应用只有集成并启动 ESP-Iris 才有该服务；Vibe 是安装维护固件，也运行 ESP-Iris；ROM 是芯片下载程序，没有 ESP-Iris。因此“有 USB 端口”“设备开机”“Iris 可连接”均不能单独证明 red-green 应用正在工作。[官方运行模式][modes]

版本信息存在不同层级，不能混成一个“ESP-IDF 都支持”：

- 芯片 FAQ 说明 master/v6.1 已有支持，并将 v6.1.1 及以后列为正式支持 tag；Developer Portal 页面仍使用 preview 表述，应以选定源码版本与功能表核对。[ESP32-S31 FAQ][faq]、[支持入口][status]
- 本次读取公共 BSP 的 HEAD 为 `716fc7bca30ed9c61166a0343bf6f5f995b5131f`，其 `idf_component.yml` 要求 **ESP-IDF >= 6.2**。部分示例 README 还写 >=6.0，实际依赖约束应优先按 manifest 处理，不可只读旧示例文字。[固定版本 BSP manifest][manifest]
- 官方 Vibe 工作区 README 则固定 ESP-IDF master 到 `7b9cc1ac79f865983f59bb8ff3ff43eb74ff1dbe`。若走这条路径，应遵循工作区已固定的依赖组合，不能随意套另一版本 BSP。[Vibe README][vibe]
- ESP-Brookesia 是另一套适配，V1.2 的验证与默认选项具有自身边界，不能把其“接口存在”当成所有音频、存储、USB 行为都已验证。[Brookesia 适配][brookesia]

## 5. 对 red-green 的适配建议（待用户决定）

| 维度 | USB-C 数据连接 | 局域网 Wi-Fi（Station） |
| --- | --- | --- |
| 安装与日常使用 | 同一根线供电与传状态，无需配网 | 先配网和发现/绑定设备；可用 USB 仅供电 |
| 断线判定 | 可利用拔线/端口断开，但仍需要应用心跳 | 需要心跳超时及重连，设备或网络故障不易仅凭 socket 区分 |
| 使用环境 | 适合固定电脑旁设备；受线缆和 USB Hub 影响 | 摆放更自由；依赖 2.4 GHz、IP 可达及网络隔离策略 |
| 开发重点 | CDC/Iris 接口、日志隔离、控制线、身份和重枚举 | 配网、身份绑定、发现、连接鉴权与网络恢复 |
| 带宽 | 小量状态足够，不需要将 HS 带宽作为选型理由 | 小量状态同样足够，无需串流图片或叫声 |

上表是根据已确认硬件/软件能力及需求得出的工程比较，不是实测延迟、功耗或稳定性结论。

推荐电脑继续负责会话采集、有效状态、已阅、前台静默及发声裁决；设备负责本地动画、用量显示和预置音频播放。每次重连先发完整快照，播放命令带去重标识，避免补发旧叫声。项目已有 manager 集中裁决、显示端渲染和预生成叫声的结构，可作为适配基础。[项目说明](../../README.md)、[现有协议](../protocol.md)、[叫声决策](../adr/0002-prerendered-call-assets.md)

“设备运行时独占显示和声音，不运行时降级电脑”应基于 **red-green 应用身份/协议版本/资源版本握手 + 应用级心跳**，而非仅看 USB 或网络连接。设备运行其他 Idea、Vibe 或 ROM 时，电脑应继续显示；进入设备模式后需同时停止电脑宠物窗口、用量表和本地叫声。短暂断线的宽限、恢复时输出交接及重复事件语义需在实施前明确。本段是需求落地建议，未修改产品代码。

## 6. 真机验证清单

1. 确认 CoreBoard 丝印 V1.0/V1.2、出厂固件/当前 Idea、eFuse 板型与实际 SDK/BSP 组合。
2. 在 macOS 上确认 USB 的设备描述符、接口/端口、稳定 Device ID；验证正确应用握手。不要把文档里的 Linux `/dev/ttyACM0` 当 macOS 固定路径。
3. 验证端口打开/关闭、DTR/RTS、调试工具并用、拔插、Hub、睡眠唤醒、应用复位后是否自动重枚举；业务帧是否与日志隔离。尤其核对所选固件自动下载行为。
4. 同时播放动画、更新用量并播放叫声，测内存、帧率、音频质量与持续运行；确认 V1.2 音频及存储实际表现，不只依据 API 初始化成功。
5. 验证设备断电、固件卡住、切换 Idea/Vibe/ROM、网络中断时电脑恢复显示/声音；恢复时不双播、不补叫旧事件。
6. 如果选 Wi-Fi，验证目标网络的 2.4 GHz、设备发现、鉴权、路由器隔离、IP 变化、电脑睡眠及重连。若选蓝牙，再开展对应 macOS 协议验证。

本次工作只核查文档与源码，不能替代上述运行验证；尤其不承诺现有出厂固件可零修改接入。

[board12]: https://docs.espressif.com/projects/esp-dev-kits/zh_CN/latest/esp32s31/esp-mosaico/user_guide.html
[board10]: https://docs.espressif.com/projects/esp-dev-kits/zh_CN/latest/esp32s31/esp-mosaico/user_guide_v1.0.html
[usj]: https://github.com/espressif/esp-idf/blob/master/docs/en/api-guides/usb-serial-jtag-console.rst
[firmware]: https://mosaico.ai/zh-hans/firmware
[support]: https://developer.espressif.com/persist/chip-support-status/esp32s31.json
[status]: https://developer.espressif.com/hardware/esp32s31/
[tinyusb]: https://github.com/espressif/esp-usb/blob/master/device/esp_tinyusb/idf_component.yml
[brookesia]: https://docs.espressif.com/projects/esp-brookesia/en/latest/hal/boards/espressif.html
[factory]: https://mosaico.ai/zh-hans/guide
[wifi]: https://docs.espressif.com/projects/esp-idf/en/latest/esp32s31/api-guides/wifi-driver/overview.html
[bluetooth]: https://docs.espressif.com/projects/esp-idf/en/latest/esp32s31/api-reference/bluetooth/index.html
[camera]: https://docs.espressif.com/projects/esp-dev-kits/zh_CN/latest/esp32s31/esp-mosaico/user_guide_camera.html
[interact]: https://docs.espressif.com/projects/esp-dev-kits/zh_CN/latest/esp32s31/esp-mosaico/user_guide_interact.html
[bsp]: https://github.com/esp-mosaico/esp-mosaico-bsp
[audio]: https://github.com/esp-mosaico/esp-mosaico-bsp/blob/716fc7bca30ed9c61166a0343bf6f5f995b5131f/examples/gmf_audio_player/README.md
[gateway]: https://github.com/esp-mosaico/esp-mosaico-vibe/blob/main/docs/project-gateway.md
[modes]: https://github.com/esp-mosaico/esp-mosaico-vibe/blob/main/docs/device-modes.md
[screen]: https://github.com/esp-mosaico/esp-mosaico-bsp/blob/716fc7bca30ed9c61166a0343bf6f5f995b5131f/examples/usb_extend_screen/README.md
[faq]: https://esp32-s31.espressif.com/en/faq
[manifest]: https://github.com/esp-mosaico/esp-mosaico-bsp/blob/716fc7bca30ed9c61166a0343bf6f5f995b5131f/components/esp-mosaico-bsp/idf_component.yml
[vibe]: https://github.com/esp-mosaico/esp-mosaico-vibe/blob/main/README.md
