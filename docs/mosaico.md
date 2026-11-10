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

字体使用 Noto Sans CJK SC（SIL OFL，许可保留在生成目录），20/28 px、1 bit，覆盖字体中 U+0020–FFFF 的可用字形。执行 `cd app && pnpm install --frozen-lockfile` 后，`bash scripts/gen-mosaico-fonts.sh` 可从校验 SHA256 的固定上游提交重建。**超出字集的项目名会拒绝设备快照并回退电脑**，不显示方框。字体约 5.3 MiB，完整应用约 7 MiB；能否装入实物应用槽位仍须只读检查。

原生预览运行与固件相同的 LVGL 界面代码，支持检查中文长名称和五行总览；不模拟物理屏幕、codec 或 USB：

```sh
cmake -S firmware/mosaico/tests/preview -B /tmp/rg-preview -G Ninja
cmake --build /tmp/rg-preview -j 8
/tmp/rg-preview/preview firmware/mosaico/tests/preview/five.json /tmp/rg-five.ppm 2000
```

需先通过固件配置取得锁定的 LVGL 依赖。`sh firmware/mosaico/tests/run.sh` 在主机使用 ASan/UBSan 验证授权期限、去重、过期、帧上限与轮播公平性。`node --test app/tests/*.test.js` 验证资源可重跑及设备用量投影。

设备授权到期或显示/音频无进展时，高优先级任务关闭功放并切回等待画面；显示锁无法取得则拉低屏幕复位并重启。该软件路径已编译和模拟验证，硬故障下物理屏幕/声音的实际截止时间待真机测量。

Claude 的 `updated_at` 表示数值最后变化时间，设备显示“数值更新于 …”；没有可信 `resets_at` 时显示未知，到重置时刻后标为过期。Codex 继续使用成功快照 120 秒时效。

## 安装边界

`partitions.csv` 只用于编译容量校验，不是覆盖实物的授权。首次安装前读取真实板型、现有分区和闪存大小，保存可恢复镜像，核对应用槽位足够大后才能写入应用。禁止全片擦除、改写 eFuse，或直接运行会覆盖 bootloader/partition table 的默认 `idf.py flash`。

现有 Vibe/Iris 与维护分区必须保留；实际恢复和应用部署命令在只读检查后按实物记录。
