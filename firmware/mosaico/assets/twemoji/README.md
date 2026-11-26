# Mosaico 气泡图形

图形来自 **Twemoji**，作者 Twitter, Inc. 及其他贡献者，采用 [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)；完整许可见 [LICENSE-GRAPHICS](LICENSE-GRAPHICS)。

上游：[jdecked/twemoji v17.0.3](https://github.com/jdecked/twemoji/tree/b6b55fef1e8636b540a6d016a4729ca8cdf2e60b/assets/svg)，固定提交 `b6b55fef1e8636b540a6d016a4729ca8cdf2e60b`。本目录保留七个原始 SVG。

使用方式：💤 空闲，🚩❗ 待确认，💥 异常中止，👋💬 轮到你，✨ 已完成。`scripts/gen-mosaico-assets.mjs` 使用锁定的 `@resvg/resvg-js` 离线转换为 32×32 像素透明图片，组合符号间距 4 像素，再转为 LVGL ARGB8888。除栅格化、尺寸调整和组合排列外未修改图形。原始 SVG、生成脚本与转换结果均参与资源版本校验。

重建：在 `app/` 执行 `pnpm install --frozen-lockfile`，然后从仓库根目录执行 `node scripts/gen-mosaico-assets.mjs`。设备及对应桌面应用必须使用同一份生成资源版本。
