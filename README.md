# ADM 下载器

ADM 是一个面向 Windows 的轻量级 HTTP/HTTPS 下载器，使用 Tauri、Rust、Vue 3 和 TypeScript 开发。

项目最初源于对现有下载工具界面和功能体积的不满，因此主要目标是：

- 界面简洁、美观
- 支持多连接分段下载
- 尽量减少常驻进程的资源占用
- 提供浏览器下载接管能力
- 保持本地配置和任务数据可控

## 功能

- 支持 HTTP / HTTPS 下载
- 多连接分段下载
- 暂停、继续和断点续传
- 下载任务队列与并行任务调度
- 自定义下载目录
- 下载进度、速度和剩余时间显示
- 下载限速
- 系统托盘
- 浏览器扩展下载接管
- 下载错误与任务状态提示
- Windows 便携版运行

## 当前状态

当前版本为 `0.1.0`，主要面向 Windows 和 Microsoft Edge。

项目暂未提供开源许可证，默认保留全部权利。源码公开主要用于学习、测试和个人备份。

## 技术栈

- Tauri 2
- Rust
- Vue 3
- TypeScript
- Vite
- Element Plus
- Windows Native TLS

## 目录结构

```text
.
├─ extension/          浏览器扩展
├─ icon/               应用图标
├─ scripts/            构建和辅助脚本
├─ src/                前端源码
├─ src-tauri/          Rust 后端及 Tauri 配置
├─ index.html          主窗口入口
├─ popup.html          扩展弹窗页面
├─ package.json        前端依赖与命令
└─ vite.config.ts      Vite 配置
