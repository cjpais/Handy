该目录提供了 `WindowBody.xaml.cs` 中 `InitDevice()` 的 Rust 抽象版。

映射关系：

- `USBHelper.GetHidMouserIDList()` -> `UsbHidProvider::get_hid_mouse_ids`
- `Global.GetManufacturerGetDevicePid()` -> `ManufacturerResolver::get_pid_vid`
- `Global.GetManufacturerById()` -> `ManufacturerResolver::get_device_type`
- `Global.GetManufacturerUsb()` -> `ManufacturerResolver::populate_usb_manufacturer`
- `BleDeviceInit.Start()` -> `BluetoothProvider::start_service`
- `BleDeviceInit.Mousetype` -> `BluetoothProvider::mouse_type`
- `YZW_HOGP_Usage.FindFirstBleDeviceIdAsync()` -> `BluetoothProvider::find_first_ble_device_id`
- `YZW_HOGP_Usage.CreateAndInitFromDeviceIdAsync()` -> `BluetoothProvider::create_and_init_from_device_id`
- `Moser_HID_Startup.HIDStartup()` -> `HidStarter::hid_startup`

入口：

- `src/device_initializer.rs` 中的 `DeviceInitializer::init_device`

Windows 适配层：

- `src/windows_impl/usb_helper.rs` -> `WindowsUsbHidProvider`
- `src/windows_impl/manufacturer_resolver.rs` -> `WindowsManufacturerResolver`
- `src/windows_impl/ble_device_init.rs` -> `WindowsBleDeviceInitAdapter`
- `src/windows_impl/yzw_hogp_usage.rs` -> `WindowsHidStarter`
- `src/windows_impl/logger.rs` -> `StdoutLogger`

示例：

```rust
use aimouse_device_init::{DeviceInitializer, InitDeviceContext};
use aimouse_device_init::windows_impl::{
    StdoutLogger, WindowsBleDeviceInitAdapter, WindowsHidStarter,
    WindowsManufacturerResolver, WindowsUsbHidProvider,
};

let initializer = DeviceInitializer::new(
    WindowsUsbHidProvider::default(),
    WindowsManufacturerResolver::with_default_rules(),
    WindowsBleDeviceInitAdapter::default(),
    WindowsHidStarter::default(),
    StdoutLogger,
);

let mut context = InitDeviceContext::default();
let report = initializer.init_device(&mut context)?;
println!("final mode: {:?}", report.final_mode);
```

说明：

- 当前 Windows 适配层优先保证 Rust 侧结构完整、可编译、可接入。
- `USBHelper` / `BleDeviceInit` 通过 PowerShell 查询当前 Windows 设备。
- `YZW_HOGP_Usage` 对应的 HID 启动器目前是可替换适配器，宿主应用可继续接入真实 HID/HOGP 实现。

---

## 已从原 WPF 中整理出的设备映射表

以下规则已从 `BeanBagAIMouse/Util/Global.cs` 中实际提取。

### `Global.GetManufacturerGetDevicePid(string manufacturerId)`

当前原 WPF 里只识别这 3 组 `VID/PID`：

| VID | PID | 说明 |
|---|---|---|
| `0x1E9D` | `0x0867` | 接收器设备，归类到 `ManufacturerID = 0` |
| `0x0D8C` | `0x0312` | 接收器设备，归类到 `ManufacturerID = 0` |
| `0x248A` | `0xC0CB` | 接收器设备，归类到 `ManufacturerID = 1` |

### `Global.GetManufacturerById(string manufacturerId)`

这个方法当前返回的 `device_type` 与 WPF 中使用方式基本等价于“设备类别/制造商类别”：

| HID InstanceId 包含 | 返回值 | 同时设置 |
|---|---:|---|
| `1E9D` + `0867` | `0` | `GlobalArguments.MouserUsb.ManufacturerID = 0` |
| `0D8C` + `0312` | `0` | `GlobalArguments.MouserUsb.ManufacturerID = 0` |
| `248A` + `C0CB` | `1` | `GlobalArguments.MouserUsb.ManufacturerID = 1` |
| 其他 | `-1` | 不匹配 |

### `Global.GetManufacturerUsb(int pid, int vid)`

这个方法按数值形式再次做同样的映射：

| VID | PID | 最终 `ManufacturerID` |
|---|---|---:|
| `0x1E9D` | `0x0867` | `0` |
| `0x0D8C` | `0x0312` | `0` |
| `0x248A` | `0xC0CB` | `1` |

### 对 Rust 侧的直接结论

因此当前 Rust 默认规则如果要对齐原 WPF，至少应补齐这 3 组：

- `VID_1E9D & PID_0867 -> device_type 0, manufacturer_id 0`
- `VID_0D8C & PID_0312 -> device_type 0, manufacturer_id 0`
- `VID_248A & PID_C0CB -> device_type 1, manufacturer_id 1`

注意：

- 原 WPF 代码里是 `C0CB`
- 不是之前 README 中写的 `C0BB`

这意味着当前 Rust 规则表如果仍然用 `248A/C0BB`，就和原 WPF **不一致**，需要修正。

---

## `Moser_HID_Startup.HIDStartup()` 调用链整理

以下调用链已经从当前 WPF 代码中拆出。

### 1. 入口调用位置

在 `WindowBody.xaml.cs` 的 `InitDevice()` / `InitDevicePro()` 中：

```csharp
Moser_HID_Startup.HIDStartup((ushort)vid, (ushort)pid, GlobalArguments.MouserUsb.ManufacturerID);
```

触发前置流程为：

1. `USBHelper.GetHidMouserIDList()` 获取所有 HID ID
2. `Global.GetManufacturerGetDevicePid()` 提取 `VID/PID`
3. `Global.GetManufacturerById()` 推导 `device_type`
4. `Global.GetManufacturerUsb(pid, vid)` 设置 `GlobalArguments.MouserUsb.ManufacturerID`
5. 调用 `Moser_HID_Startup.HIDStartup(...)`

### 2. `HIDStartup()` 内部逻辑

根据 `BeanBagAIMouse/Util/Moser_HID_Startup.cs`：

```csharp
public static void HIDStartup(ushort vid, ushort pid, int Manufacturerdata)
{
    Manufacturer = Manufacturerdata;
    CDProcess ??= new ClassDataProcess();
    CDProcess.isConnectedFunc = ConnectedStatus;
    CDProcess.pushReceiveData = DataReceived;
    CDProcess.Initial(vid, pid);
}
```

也就是说它本质做了 4 件事：

1. 保存当前 `Manufacturer`
2. 初始化 `ClassDataProcess`
3. 绑定连接状态回调：`ConnectedStatus`
4. 绑定收包回调：`DataReceived`
5. 调用 `CDProcess.Initial(vid, pid)` 打开 HID

### 3. 连接建立后的链路

`ConnectedStatus(bool isConnected)` 中：

- 连接成功时记录日志 `HID已连接`
- 延迟约 `4s`
- 如果 `GlobalArguments.MouserUsb.ManufacturerID == 11`
  - 调用 `SendBytesZYTD()`

说明：

- 不同制造商后续可能要发初始化命令
- Rust 侧如果要完整复刻，不能只“打开 HID”，还要支持“连接成功后的制造商特定初始化指令”

### 4. 收包后的链路

`DataReceived(byte[] data)` 中负责：

- 判断当前是 `BLE` 还是 `HID`
- 归一化 BLE 包格式
- 解析按键操作码
- 处理语音键 / AI 键 / M 键
- 音频流帧解析
- 调用：
  - `MouseRecordingStart()`
  - `MouseRecordingStop()`
  - `M_keyAllocationExecution.execute()`
  - `Global.Body.SendMessageToJavaScript(...)`

这说明 Rust 侧未来若要完全替代 WPF 中这一层，至少还缺：

- HID 原始收包
- 数据包解析
- 制造商分支逻辑
- 录音开始/停止指令
- BLE/HID 共用的操作码标准化

---

## 当前还缺少的关键内容

现在还缺两样关键东西，需要明确：

### 1. 缺“目标鼠标”的匹配规则

当前 `HID-BLE-Rust` 默认只认识很少的规则，核心只有：

- `VID_248A & PID_C0CB`（修正后）
- `VID_1E9D & PID_0867`
- `VID_0D8C & PID_0312`

但这台机器当前枚举到的 HID 设备是：

- `VID_04A5 & PID_8002`
- `VID_0B05 & PID_1939`
- `VID_3554 & PID_FA09`
- 以及若干 generic `HID-compliant mouse`

这意味着：

- 线程已经会扫描
- 但如果目标鼠标不在现有规则表里，它不会被判定为“匹配鼠标”

因此还需要补充：

- 目标鼠标对应的 `VID/PID`
- 或者原 WPF 中完整的 `Manufacturer / PID / VID / device_type` 映射表

只有拿到这些规则后，Rust 版本才能正确识别“哪一个 HID 设备才是目标鼠标”。

### 2. 缺真正的 HID 启动实现

`HID-BLE-Rust` 当前这层仍然是空壳：

- `src/windows_impl/yzw_hogp_usage.rs`

也就是说：

- 现在已经能做“发现 / 检测”
- 但还不能做原 WPF 里完整的 `HID / HOGP` 连接启动流程

当前还缺：

- 原 WPF 中 `Moser_HID_Startup.HIDStartup()` 的实际逻辑迁移
- 或者对应 HID 回调、收包、连接状态、设备初始化指令发送的完整实现

---

## 额外说明

还有一个与本次接入无关的仓库遗留问题：

- `src-tauri/src/bin/handy-stt.rs`

当前它本身编不过，原因包括：

- 缺少 `base64` 依赖
- `transcribe-rs` API 不匹配

但这 **不影响主应用**，也 **不影响这次 HID 监测线程的接入**。

---

## 下一步需要提供的信息

只要提供以下任意一种信息，就可以继续把 Rust 侧补完整：

1. 目标鼠标对应的 `VID/PID`
2. 原 WPF 的更完整设备映射表（如果还有别处）
3. `ClassDataProcess.Initial()` / `SendBytes()` / 回调绑定的实现位置
4. 原 WPF 里 `Moser_HID_Startup.DataReceived()` 需要优先迁移的那一部分逻辑
