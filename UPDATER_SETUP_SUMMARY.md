# Tauri 应用打包成exe配置总结

本文档总结了为 Handy 应用打包所进行的所有修改和设置。

### 第一阶段：VulkanSDK版本1.4.xxx兼容性问题

前往https://vulkan.lunarg.com/sdk/home下载1.3.280.0版本

安装后添加系统环境变量：变量名VULKAN_SDK，路径为C:\VulkanSDK\1.3.280.0

同时打包窗口同步设置环境变量如下

```
set VULKAN_SDK=C:\VulkanSDK\1.3.280.0

set CMAKE_PREFIX_PATH=%VULKAN_SDK%
```

**文件**: `src-tauri/Cargo.toml`

添加了 Vulkan SDK 环境变量，解决了构建时的依赖问题：

```toml
[env]
VULKAN_SDK = { value = "C:\\VulkanSDK\\1.4.328.1", force = true }
```

### 2. 代码签名配置



### 第二阶段：底层编译错误（C++ 编码冲突）

*   **报错现象**：
    在编译 `whisper-rs-sys` 依赖库时，出现大量的 `warning C4819`（字符无法表示）和 `error C3688`（文本后缀“銆”、“鈾”无效）。
*   **原因分析**：
    这是 Windows 开发经典问题。你的 Windows 系统默认编码是 **GBK**（中文），而开源库 `whisper.cpp` 的源码是 **UTF-8** 编码。微软编译器（MSVC）默认用 GBK 去读 UTF-8 文件，导致代码里的特殊符号被误读为乱码汉字，从而编译失败。
*   **解决方法**：
    打包窗口设置环境变量，强制编译器使用 UTF-8 读取源码：
    
    ```cmd
    set CFLAGS=/utf-8
    set CXXFLAGS=/utf-8
    ```
    并执行 `cargo clean` 清理旧缓存后重新编译，**编译成功通过**。
    
    ```cmd
    cd  C:\Server\183\Handy\src-tauri
    cargo clean
    cd  C:\Server\183\Handy\src-tauri
    ```
    
    

---

### 第二阶段：打包配置错误（Windows 代码签名）

*   **报错现象**：
    编译完成后，在打包阶段提示 `failed to bundle project: program not found`。
*   **原因分析**：
    `tauri.conf.json` 的 `bundle > windows` 配置中保留了 `signCommand`（通常调用 `signtool.exe`），但你的电脑环境变量中没有这个工具，或者你并不需要购买昂贵的 Windows 数字证书。
*   **解决方法**：
    直接编辑 `tauri.conf.json`，**删除** `signCommand` 这一行配置。

#### Windows 签名配置 (tauri.conf.json)

**文件**: `src-tauri/tauri.conf.json`

**修改前**:

```json
"windows": {
  "signCommand": "trusted-signing-cli -e https://eus.codesigning.azure.net/ -a CJ-Signing -c cjpais-dev -d Handy %1"
}
```

**修改后**:

```json
"windows": {}
```

**原因**: 移除了需要外部签名服务的配置，改用本地签名。

---

### 第三阶段：更新签名错误（Tauri Updater）

*   **报错现象**：
    安装包已经生成了（`.msi` 和 `.exe` 都在了），但最后一步报错：`A public key has been found, but no private key`。
    
*   **原因分析**：
    项目启用了 **Tauri 自动更新插件 (Updater)** 并在配置文件里填了公钥 (`pubkey`)。Tauri 试图为生成的安装包制作更新签名文件，但找不到对应的**私钥**。
    
*   **尝试过的弯路（“硬删除”法）**：
    我们尝试去删除 `pubkey`、删除 `updater` 配置块、注释 Rust 代码、修改权限文件。
    *   *结果*：由于 Tauri v2 的各个模块（Cargo features、Config、Capabilities、Rust Code）耦合紧密，删了一个地方，另一个地方就会报错（如 `plugins > updater doesn't exist`），导致陷入“拆东墙补西墙”的困境。
    
*   **最终解决方法（“顺从”法）**：
      2. **生成密钥**：运行 `npm run tauri signer generate` 生成新的公钥/私钥对（密码留空）。

使用 Tauri CLI 生成了新的 RSA 密钥对：

```bash
npm run tauri signer generate
```

生成的密钥：

- **私钥**: `dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5ClJXUlRZMEl5K08xSng2L05WWjRjSXZkTnU2R3ZIWWVtcVN5ckRETXBCWkJ5WlFkMzZjRUFBQkFBQUFBQUFBQUFBQUlBQUFBQVNqM3VLalFqd2VleWtBNHZJUEU0amE1RGU4bFlaUFkvZWE3bTg5NlJjUFNmb0w0aHZUdVM4cjhHdXNId0tSWFBoZG1DMDNYS3EwQ1JEbzViQ3FRRmtpUktCR0pZeXNNYy9CRkZLSFptdjZOamhONm9JTTl5M0lFMnpVOFppaG5pT2pqSDBJcDg1UjA9Cg==`
- **公钥**: `dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEU3QzEyMzM0MkFFRTNENEEKUldSS1BlNHFOQ1BCNTc4R3VzSHdLUlhQaGRtQzAzWEtxMENSRG81YkNxUUZraVJLQkdKWXlzTWMK`

#### 更新配置文件 (tauri.conf.json)

**修改前**:

```json
"plugins": {
  "updater": {
    "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEJBQjcyMDk1MjA2NjAxRjkKUldUNUFXWWdsU0MzdXRRZi8zYzhqV2FaNUVDbDd2Rk5VM1IvWWowVXdmRFNKQ1BrMXF5RFFsLy8K",
    "endpoints": [
      "https://github.com/cjpais/Handy/releases/latest/download/latest.json"
    ]
  }
}
```

**修改后**:

```json
"plugins": {
  "updater": {
    "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEU3QzEyMzM0MkFFRTNENEEKUldSS1BlNHFOQ1BCNTc4R3VzSHdLUlhQaGRtQzAzWEtxMENSRG81YkNxUUZraVJLQkdKWXlzTWMK",
    "endpoints": [
      "https://github.com/cjpais/Handy/releases/latest/download/latest.json"
    ]
  }
}
```



## 自动更新机制说明

### 工作原理

1. **检查更新**: 应用启动时检查 `https://github.com/cjpais/Handy/releases/latest/download/latest.json`
2. **版本比较**: 比较当前版本与远程版本
3. **下载更新**: 如果发现新版本，下载对应的更新包
4. **签名验证**: 使用内置的公钥验证更新包的签名
5. **安装更新**: 验证通过后，提示用户并安装更新

### 安全性

- **私钥**: 仅开发者持有，用于签名更新包
- **公钥**: 内嵌在应用中，用于验证更新包真实性
- **签名验证**: 确保更新包未被篡改
- **HTTPS传输**: 所有通信均使用加密连接

## 构建和发布流程

### 开发者发布更新时

```bash
# 设置VULKAN_SDK环境变量
set VULKAN_SDK=C:\VulkanSDK\1.3.280.0
set CMAKE_PREFIX_PATH=%VULKAN_SDK%

# 设置私钥环境变量
export TAURI_SIGNING_PRIVATE_KEY="你的私钥内容"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""  # 如果密码为空

# 清除编译缓存
cd  C:\Server\183\Handy\src-tauri
cargo clean
cd  C:\Server\183\Handy\src-tauri

# 构建并签名
npm run tauri build

# 上传到 GitHub Releases
# 更新 latest.json
```

### 关键文件列表

1. **配置文件**:
   - `src-tauri/Cargo.toml` - 依赖和环境变量配置
   - `src-tauri/tauri.conf.json` - 主配置文件
   - `src-tauri/capabilities/default.json` - 默认权限配置
   - `src-tauri/capabilities/desktop.json` - 桌面权限配置

2. **源代码**:
   - `src-tauri/src/lib.rs` - 主应用逻辑和插件初始化

3. **重要信息**:
   - **私钥**: 需要妥善保管，用于签名
   - **公钥**: 已配置在 tauri.conf.json 中
   - **更新端点**: `https://github.com/cjpais/Handy/releases/latest/download/latest.json`

## 注意事项

1. **私钥安全**:
   - 永远不要将私钥提交到代码仓库
   - 考虑使用密码保护私钥
   - 定期备份私钥

2. **发布流程**:
   - 每次发布新版本时都要使用私钥签名
   - 及时更新 `latest.json` 文件
   - 确保 GitHub Releases 中的文件完整

3. **测试验证**:
   - 在发布前测试自动更新流程
   - 验证签名是否正确
   - 测试不同平台的更新过程

---

**创建日期**: 2025-01-10