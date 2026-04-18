# `Moser_HID_Startup.cs` -> Rust 迁移说明

已在 `src/moser_hid_startup.rs` 提供可集成版本，核心目标是迁移 `DataReceived` 处理流程（按你的要求，`Manufacturer == 0` 分支不处理）。

## 1. 已迁移范围

- BLE/HID 模式判断
- BLE 包头 `0xCC 0x3C` 规范化
- `idx1/idx2` 动态索引选择
- `Manufacturer == 1` 分支核心逻辑
  - 音频帧识别（`0x3C`）
  - ADPCM 60 字节提取并回调解码
  - 按键事件：`32/1`、`32/3`、`32/4`、`34/1`、`34/3`、`34/4`、`35/3`、`35/4`、`48/1`、`48/3`、`48/4`
- 防抖逻辑（400ms）

## 2. 主动未迁移

- `Manufacturer == 0` 的全部 UI/JS 交互流程（按要求跳过）
- C# 全局静态对象直接访问（例如 `Global`、`Arguments`、`UserSettings`）
  - 已改成 `HandlerConfig` 输入 + `MoserHost` 回调接口

## 3. 与宿主项目集成方式

你需要在 Rust 宿主项目里实现 `MoserHost`：

- `send_bytes_mouse_recording_start/stop`
- `mouse_recording_start/stop`
- `m_key_execute` / `m_key_execute_on_click`
- `open_main_window` / `close_main_window`
- `decode_adpcm_to_pcm`
- `append_pcm`

然后：

1. 构造 `MoserHidStartupHandler`
2. 每次收到 HID/BLE 包时调用 `data_received(data, host)`
3. 更新 `handler.config` 里的运行态（连接模式、按钮功能、M 键模式等）

## 4. 文件

- 核心实现：`src/moser_hid_startup.rs`
- 对外导出：`src/lib.rs`

