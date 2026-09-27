# SharbCut

SharbCut 是基于 [Concat](https://github.com/jub0t/Concat) 开发的 Windows 桌面视频编辑器。普通剪辑、BGM 自动卡点和 AI 剪辑都作用于同一条可编辑时间线；生成的镜头保留对原素材的引用，可以继续手动修改、撤销和重做。

当前为测试版本。项目保留原 Concat 的多轨时间线、预览、字幕、转场、效果与导出功能；新增的 AI Agent 和 BGM-Montage 不需要单独导入渲染好的成片。

## 安装与使用

- Windows x64 安装包：`release/SharbCutSetup.exe`。安装包已包含应用、FFmpeg、BGM-Montage 所需的 Python 运行时和依赖；使用者不需要安装 Rust、Cargo、CMake 或 FFmpeg。
- 便携版：解压 `release/SharbCut-Windows-x64.zip`，运行其中的 `SharbCut.exe`。便携包的 `portable` 文件夹保存本机设置与下载的模型。
- 新建项目并导入视频、音频后，可以像普通编辑器一样手动剪辑和导出。原素材需保持在原位置，项目时间线引用这些文件，并不复制全部媒体。

### BGM 自动卡点

在媒体区选择 BGM 和素材，使用“自动卡点”。默认模式是 **BMTS-lite**，适合日常快速生成；“完整模式”是可选的高级分析流程，通常更慢。两种模式都应将源素材片段和 BGM 直接放进当前可编辑时间线，并允许撤销、重做。也可以使用“导入卡点时间线”导入兼容的 `edit_decisions.json`。

### AI 剪辑

打开顶部“AI 剪辑”，填写兼容 OpenAI Chat Completions 的 Base URL、模型及 API Key，然后用中文描述对当前项目的修改，例如“把第一段改成 1.5 倍速”或“用已导入的 BGM 自动卡点”。AI 返回的操作经过命令白名单和项目校验，再作为可撤销的编辑应用到当前时间线。API Key 保存在 Windows 凭据管理器，不写入项目文件。

**数据边界：** 发起 AI 剪辑请求时，应用会向你配置的模型服务发送项目/时间线元数据、素材路径与名称，以及最多 4 帧低分辨率预览；选中素材时可能附带音量摘要和已安装转写模型产生的短对白。若 Base URL 指向外部服务，这些数据会离开本机。普通本地剪辑和 BGM 自动卡点不需要 AI 服务。请自行确认所用模型服务及素材的隐私要求。

## 开发

在 Windows x64、MSVC 构建环境中运行：

```powershell
.\scripts\setup-dev.ps1
Set-Location .\engine
cargo check -p concat --locked
cargo build -p concat --locked
Set-Location ..
.\scripts\run-dev.ps1 -SkipBuild
```

首次运行 `setup-dev.ps1` 会在项目的 `.tools` / `vendor` 中准备可项目级存放的工具和依赖；MSVC / Windows SDK 等系统组件仍需正常安装。日常验证使用 BMTS-lite 和小规模真实素材，不运行完整模式。生成 Windows 发布包：

```powershell
.\scripts\package-windows.ps1
```

## 来源与许可

SharbCut 基于 Concat；原项目的许可证及附加许可见 [LICENSE](LICENSE) 和 [LICENSE-EXCEPTIONS.md](LICENSE-EXCEPTIONS.md)。BGM-Montage、BMTS-lite、FFmpeg、Slint 等第三方组件及许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 与安装包中的相应许可文件。原 Concat 项目的开发文档仍保留在仓库中，部分页面尚使用原名称。
