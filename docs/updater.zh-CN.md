# 更新 OctoSense

[English](updater.md) | 简体中文

在桌面版或 Home 启动器中打开 **OctoSense Updates**。Android Home 也提供
**Settings → Updates → Open OctoSense app updater** 入口。先选择通道，再点
**Check for updates**。查看版本号与下载大小，点 **Download update**；校验完成
后，还需单独操作才能打开安装程序。

| 通道 | 可选版本 |
| --- | --- |
| Stable（稳定版） | 仅稳定版。 |
| Release candidate（候选版） | 稳定版和 `rc` 预发布；排除 beta、alpha、nightly。 |
| Preview（预览版） | 稳定版及所有符合语义版本格式的预发布版。 |

已安装 RC 时默认选择候选版通道。版本按数值比较：`rc.10` 高于 `rc.9`，相应
正式版高于其 RC。切换通道不会允许降级。

更新器随本次源代码变更引入。已发布的桌面版（至 `0.1.0-rc.5`）和 Home
`0.1.0-beta.2` 不含此功能；需要先手动安装一次包含更新器的新版本。没有发布
标签的开发构建无法确定当前安装的发行版，因此不提供更新。

## 安装

| 平台 | 交接方式 | 用户还需完成的操作 |
| --- | --- | --- |
| macOS，Apple silicon | 打开已校验的 `.dmg` | 退出 OctoSense，将新应用拖入 Applications，确认替换，再打开。 |
| Windows，x64 | 直接启动已校验的 NSIS `.exe` | 按提示安装，在要求时关闭 OctoSense。 |
| Linux，x86_64，Debian 包 | 把已校验的 `.deb` 复制到“下载”文件夹并再次校验，再用桌面包处理程序打开 | 在包管理器中确认安装并重启。没有图形包处理程序时，用发行版的包管理器安装该文件。 |
| Linux，x86_64，AppImage | 把校验过的可执行副本放进“下载”文件夹，并打开该文件夹 | 退出 OctoSense，用新文件替换旧 AppImage，再打开。更新器不覆盖运行中的镜像。 |
| Android，ARM64 | 对照已安装 Home 检查 APK，通过 Android 包安装程序暂存 | 查看并确认操作系统的安装提示。 |

打开安装程序**不代表**安装完成。首个版本不会定期检查、静默退出、自行提权、
覆盖运行中的应用或删除用户配置。操作系统的安装程序仍会执行自身检查，但
macOS 和 Windows 不会像对浏览器下载的文件那样显示警告（Gatekeeper、
SmartScreen）：更新器不把下载标记为来自互联网，而是依靠发布上的 GitHub 签名
（见[校验与隐私](#校验与隐私)）。

Linux 包处理程序可能读不到下载所在的隐藏配置目录（Ubuntu 的 App Center 是
snap 应用，看不到隐藏文件夹），因此 Linux 包从“下载”文件夹交接
（`XDG_DOWNLOAD_DIR`，否则为 `~/Downloads`）。那里的同名文件不会被替换：副本
使用下一个可用名称，例如 `octosense_0.2.0_amd64 (2).deb`。“下载”中的副本由您
自行删除。

需要 Android 13（API 33）或更新版本。Android 仅更新**独立版 Home**，System Bridge 与 ROM 继续使用各自的更新途径。
已安装包与候选包必须使用公开独立版 Home 的签名身份；系统内置 Home、ROM
签名、隔离测试包或其他签名均会被拒绝。候选包的 Android 版本代码必须更高，
最低 Android 版本必须兼容。若尚未允许 OctoSense 安装应用，用户可以打开系统
权限设置，返回后再次明确点击安装。更改权限后不会自动恢复安装，也不会先卸载
Home 来绕过签名不匹配。**Review and install update** 和 **Allow updates in
Android Settings** 只响应真实的屏幕点按：这由 Makepad 的输入来源判断，脚本、
远程控制或模拟点击都无法伪造。

## 校验与隐私

宿主从 GitHub 的 `OctoSense-org/OctoSense` 读取公开发布。桌面版选择
`desktop-v…` 标签，Home 选择 `home-v…`。语义版本与确切的平台文件名决定候选
更新；发布时间先后不决定版本高低。

发布必须同时提供 GitHub 的 SHA-256 文件摘要与 `SHA256SUMS`。更新器检查
校验和文件本身的摘要，确认其中的安装包哈希与 GitHub 元数据一致，再校验整个
下载文件及声明的大小。文件下载到私有暂存目录，交给操作系统前再次校验。无效
元数据、非预期 URL、符号链接、不完整或已修改文件均会被拒绝。

摘要和校验和只能证明文件属于该发布：任何能发布 Release 的人都能让它们彼此
一致。因此，桌面版发布只有在其 `SHA256SUMS` 还带有 **GitHub 签名**时才会被
提供。该签名由 [`sign-desktop-release`](../.github/workflows/sign-desktop-release.yml)
工作流生成，它在 `main` 上、在 `desktop-release-signing` 环境中运行，每次运行都需
该环境的审核人批准。签名是一个 Sigstore 包，作为 `SHA256SUMS.sigstore.json`
附在发布中。更新器离线校验它，方式与设备校验 App Hub 目录相同：先用 Sigstore
公共信任根校验证书、时间戳和透明日志证明；再校验 GitHub 对这次运行的声明（本
仓库及其所有者、`main` 上的该工作流、手动触发、该环境、GitHub 托管的运行器）；
最后校验签名的对象是 `<tag>.SHA256SUMS`，且摘要与实际下载的文件完全一致。没有
该签名的桌面版发布会报错，而不是被跳过。Android Home 不需要它：Android 只安装
用固定的独立版发布密钥签名的 APK。

发布列表按从新到旧读取，最多 10 页、每页 100 个发布；每页最多 16 MB（2026 年
10 月，一个发布的说明和文件列表约占 28 KB）。

桌面版下载位于 shell 的 `updates/` 目录（设置了 `OCTOSENSE_HOME` 时以其为
根目录，否则是 `~/.octosense/updates`）。Android 使用应用私有缓存中的
`octosense-updates/`。已校验的下载在最后写入一天后删除，不论是否打开过安装
程序；中断的下载在一小时后删除。删除发生在 OctoSense 启动时和每次检查前，
只删除更新器自己的、只含普通文件的 `verified-…` 和 `pending-…` 文件夹，且不
递归删除。检查发布不需要 OctoSense、GitHub 或 Google 登录。
不会发送邮件、应用内容、模型凭证或代理对话。更新器是宿主原生界面，不是
App Hub 应用或代理工具。

## 代码与发布身份

- [`crates/updater`](../crates/updater/src/lib.rs) 负责通道、版本选择、受限下载、
  取消、已校验文件和清理过期下载。其中的 [`signature.rs`](../crates/updater/src/signature.rs)
  校验桌面版发布 `SHA256SUMS` 上的 GitHub 签名，签名由
  [`sign-desktop-release.yml`](../.github/workflows/sign-desktop-release.yml) 为每个
  发布生成（见 [desktop/README.zh-CN.md](../desktop/README.zh-CN.md)）。
- [`crates/updater-ui`](../crates/updater-ui/src/lib.rs) 负责原生模块与明确的用户
  操作。网络和哈希任务不占用绘制线程。
- [`install.rs`](../crates/updater-ui/src/install.rs) 用原样参数交给桌面安装程序，
  不把发布内容插入 shell 命令。
- Android 原生扩展独立检查包名、版本、签名和文件字节，再提交需要操作系统
  用户确认的安装会话。
- [`native-apps.json`](../native-apps.json) 为桌面版和 Home 注册模块，不提供代理
  更新工具。

Cargo 通用的 `0.1.0` 包版本不是实际发行版身份，因此打包时嵌入
`OCTOSENSE_RELEASE_TAG`。[桌面打包程序](../desktop/scripts/package.py) 从显式
的 `--version-from-tag` 或 `--version` 设置标签。带标签的 `--skip-build` 必须
匹配先前构建的标签和二进制 SHA-256 收据，防止旧程序被标成新版本。普通构建会
清除继承标签。桌面构建和打包程序显式选择本机 Rust 目标，二进制与身份收据来自
`target/<host-triple>/release/`。继承的 `CARGO_TARGET_DIR`、
`CARGO_BUILD_TARGET` 或 Cargo 配置不能把构建重定向后，让打包继续读取旧的
`target/release` 程序。[Home 构建程序](../rom/scripts/build-home.py) 只对指定版本、正式
签名的独立构建设置标签；开发与 ROM 构建留空。

## 借鉴 OctosCode

原生 OctosCode 应用在 `d47fade94498` 的文档中要求手动下载 RC，并不包含 CLI
更新器。独立的 [`octoscode update` CLI](https://github.com/octos-org/octoscode/blob/8722b551a7ee653c2457f1152a636160351b9b1f/src/cmd/update.rs)
使用 `axoupdater` 更新带 cargo-dist 安装收据的程序；由 Homebrew、npm 或 Cargo
管理的安装则交还相应管理器。它的预发布选项解析确切通道标签，并测试 RC 序号的
数值顺序。OctoSense 采用这些安装归属与明确通道的原则，但把完整应用包交给原生
安装程序。与该 CLI 显式切换预发布通道不同，本界面不提供正式版降级到 RC。

[octos 内核更新器](https://github.com/octos-org/octos/blob/084baa522a508cf3af7d7ce9585868ac710eb1d9/crates/octos-services/src/updater.rs)
可备份、替换指定 CLI 二进制并回滚，但不是完整 Makepad 应用包或 Android
安装器。OctoSense 对旧发布也强制要求校验和，不采用内核更新器允许旧版缺失
校验和的兼容策略。

## 验证范围

桌面交接测试检查程序名和原样参数，不启动安装程序。打包测试检查发布身份、拒绝
旧二进制和构建计划，不生成发布包。

签名校验用一个公开的 Sigstore 包测试：它能通过密码学校验，但作为其他工作流的
签名被拒绝。被篡改的签名与透明日志证明，其他工作流、分支、签发者和信任根，任何
缺失或错误的 GitHub 声明，以及指向其他发布或其他字节的声明也都会被拒绝。签名
工作流的发布检查在已发布的 `desktop-v0.1.0-rc.5` 和 `rc.2` 上通过，并拒绝了没有
`SHA256SUMS` 的 `beta.2`。目前还没有签过名的发布，因此对已签名发布的端到端检查
仍属**未经验证**；Android 在真机上的真实点按检查也未验证。过期下载清理和 Linux
复制到“下载”文件夹在临时目录中测试；实际 Linux 交接仍属**未经验证**。

隐藏窗口中的原生 macOS 界面检查了真实的 RC1 → RC2 候选更新，下载并校验了
297 MB 磁盘镜像。故意修改隔离下载文件后，点击 **Install** 在任何安装程序打开
之前即拒绝该文件。还验证了切换通道和取消下载；实际桌面 shell 也在隔离配置中
打开了 Updates 模块并检查了真实 RC2 候选更新。
替换已安装的 macOS 应用仍属**未经验证**，Windows 与 Linux
真实安装也尚未验证。

在自有 API 35 Android 模拟器中，使用实际 Java 更新适配器和操作系统
PackageInstaller 的测试应用通过了含清理步骤在内的 25 项验收：更改安装权限后不自动重放安装；
拒绝错误签名、包名、哈希、版本代码和最低 SDK；可取消并重试可见的系统确认；
版本代码 1000 → 1001 更新保留了 UID 与模拟应用数据，替换后可恢复状态。
此验收覆盖原生适配器，**不是完整的 Rust Home 二进制**。没有改动实体手机；
从已发布 Home 构建发起的完整安装流程仍属**未经验证**。
