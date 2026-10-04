# Visual Studio Build Tools に ARM64 用の C++ ビルドツールを追加する（ARM64 版のビルドに必要）。
# 管理者の確認（UAC）が出るので、デスクトップで実行すること。
$vs = "${env:ProgramFiles(x86)}\Microsoft Visual Studio"
$setup = "$vs\Installer\setup.exe"
$a = @(
    "modify"
    "--installPath"
    "`"$vs\2022\BuildTools`""
    "--add"
    "Microsoft.VisualStudio.Component.VC.Tools.ARM64"
    "--passive"
    "--norestart"
)
# --passive のときは最初から管理者で動かす必要がある
$p = Start-Process $setup -ArgumentList $a -Verb RunAs -Wait -PassThru
"終了コード: $($p.ExitCode)  (0 または 3010 なら成功)"
