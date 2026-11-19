$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Repo = 'chewton2k/leo-cli'
$Target = 'x86_64-pc-windows-msvc'
function Fetch([string]$Url, [string]$Destination) {
    & curl.exe --fail --location --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 900 --max-filesize 1073741824 --retry 2 --output $Destination $Url
    if ($LASTEXITCODE -ne 0) { throw 'The Leo download failed. Check your connection and try again.' }
}
function Restore-Owned([string]$Old, [string]$Path) {
    try {
        if (Test-Path -LiteralPath $Old) { Copy-Item -LiteralPath $Old -Destination $Path -Force }
        else { Remove-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue }
    } catch { Write-Warning "leo could not put back $Path." }
}
function Publish-Owned([scriptblock]$Move) {
    $linkDone = $false
    try {
        & $Move $linkTemp $link
        $linkDone = $true
        & $Move $recordTemp $record
    } catch {
        $why = $_
        if ($linkDone) { Restore-Owned $linkOld $link }
        throw $why
    }
}
function Add-UserPath([string]$Folder, [scriptblock]$Write) {
    try {
        $current = [Environment]::GetEnvironmentVariable('Path', 'User')
        $entries = @($current -split ';' | Where-Object { $_ })
        if (-not ($entries | Where-Object { $_.TrimEnd('\').Equals($Folder.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) })) {
            & $Write (($entries + $Folder) -join ';')
        }
    } catch { Write-Warning "leo could not add $Folder to your user PATH. Add it yourself." }
}
if ($env:LEO_INSTALL_FUNCTIONS_ONLY) { return }
if ($env:OS -ne 'Windows_NT') { throw 'Leo is available for macOS and Windows only.' }
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -and $env:PROCESSOR_ARCHITEW6432 -ne 'AMD64') { throw 'Download Leo for Windows on x64.' }
$AppDir = if ($env:LEO_WINDOWS_APP_DIR) { $env:LEO_WINDOWS_APP_DIR } else { Join-Path $env:LOCALAPPDATA 'Leo' }
$BinDir = if ($env:LEO_INSTALL_DIR) { $env:LEO_INSTALL_DIR } else { Join-Path $AppDir 'bin' }
foreach ($path in @($AppDir, $BinDir)) {
    if (-not [IO.Path]::IsPathRooted($path) -or $path -match '["\r\n]') { throw 'Choose absolute installation folders without quotes or line breaks.' }
}
if ($env:LEO_UPDATE_PARENT_PID) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (Get-Process -Id ([int]$env:LEO_UPDATE_PARENT_PID) -ErrorAction SilentlyContinue) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Close the terminal Leo command and try the installer again.' }
        Start-Sleep -Milliseconds 100
    }
}
$running = Get-Process -Name 'leo-app' -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.Equals((Join-Path $AppDir 'leo-app.exe'), [StringComparison]::OrdinalIgnoreCase) }
if ($running) { throw 'Quit Leo before installing. Finish any recording or dictation first.' }
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('leo-install-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $installer = Join-Path $tmp 'leo-setup.exe'
    if ($env:LEO_INSTALL_ARCHIVE) { Copy-Item -LiteralPath $env:LEO_INSTALL_ARCHIVE -Destination $installer }
    else {
        $effective = & curl.exe --fail --silent --location --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 30 --output NUL --write-out '%{url_effective}' "https://github.com/$Repo/releases/latest"
        if ($LASTEXITCODE -ne 0 -or $effective -notmatch '/tag/(v\d+\.\d+\.\d+)$') { throw 'The latest Leo release could not be identified. Try again later.' }
        $tag = $Matches[1]
        $url = "https://github.com/$Repo/releases/download/$tag/leo-$Target-setup.exe"
        Fetch $url $installer
        $checksum = Join-Path $tmp 'checksum'
        Fetch "$url.sha256" $checksum
        $expected = ((Get-Content -LiteralPath $checksum -Raw).Trim() -split '\s+')[0]
        if ($expected -ne (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash) { throw 'The app download is damaged. Download it again.' }
    }
    if ((Get-Item -LiteralPath $installer).Length -gt 1073741824) { throw 'The installer is too large. Download it again.' }
    foreach ($path in @($AppDir, $BinDir)) {
        if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The installation folder is a link. Choose another folder.' }
    }
    $cli = Join-Path $AppDir 'leo.exe'
    $link = Join-Path $BinDir 'leo.cmd'
    $config = if ($env:LEO_HOME) { $env:LEO_HOME } else { Join-Path $env:APPDATA 'leo' }
    $record = Join-Path $config 'app-cli-dir'
    $linkOld = Join-Path $tmp 'old-leo.cmd'
    $recordOld = Join-Path $tmp 'old-app-cli-dir'
    try {
        if ((Test-Path -LiteralPath $record) -and ((Get-Item -LiteralPath $record).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Remove the app-cli-dir link from the Leo settings folder before installing.' }
        New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
        New-Item -ItemType Directory -Path $config -Force | Out-Null
        $content = "@setlocal`r`n@for /f `"tokens=2 delims=:`" %%a in ('chcp') do @set `"LEO_OLD_CODEPAGE=%%a`"`r`n@chcp 65001 >nul`r`n" + '@"' + $cli.Replace('%', '%%') + '" %*' + "`r`n@set `"LEO_COMMAND_STATUS=%errorlevel%`"`r`n@chcp %LEO_OLD_CODEPAGE% >nul`r`n@exit /b %LEO_COMMAND_STATUS%`r`n"
        $linkTemp = Join-Path $BinDir (".leo-link-" + [Guid]::NewGuid().ToString('N'))
        [IO.File]::WriteAllText($linkTemp, $content, [Text.UTF8Encoding]::new($false))
        $recordTemp = Join-Path $config (".app-cli-dir-" + [Guid]::NewGuid().ToString('N'))
        [IO.File]::WriteAllText($recordTemp, $BinDir, [Text.UTF8Encoding]::new($false))
        if (Test-Path -LiteralPath $link) { Copy-Item -LiteralPath $link -Destination $linkOld }
        if (Test-Path -LiteralPath $record) { Copy-Item -LiteralPath $record -Destination $recordOld }
    } catch { throw "$($_.Exception.Message.TrimEnd('.')). Nothing was changed." }
    $process = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$AppDir") -PassThru
    if (-not $process.WaitForExit(900000)) { throw 'The installer is still running. Wait for it to finish before trying again.' }
    if ($process.ExitCode -ne 0) { throw 'The app could not be installed. Reopen the downloaded installer and try again.' }
    if (-not (Test-Path -LiteralPath $cli) -or -not (Test-Path -LiteralPath (Join-Path $AppDir 'leo-app.exe'))) { throw 'The installer did not provide the Leo app and terminal command.' }
    try { Publish-Owned { param($from, $to) Move-Item -LiteralPath $from -Destination $to -Force } }
    catch { throw "Leo's new app is installed, but its terminal command could not be updated: $($_.Exception.Message.TrimEnd('.')). The previous terminal command was put back; run the installer again." }
    if (-not $env:LEO_INSTALL_SKIP_PATH) { Add-UserPath $BinDir { param($value) [Environment]::SetEnvironmentVariable('Path', $value, 'User') } }
    $old = Join-Path $BinDir 'leo.exe'
    if (-not $old.Equals($cli, [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $old)) {
        try { Remove-Item -LiteralPath $old -Force } catch { Write-Warning "leo could not remove $old. Delete it yourself so the leo command runs the new app." }
    }
    Write-Host "Leo is installed in $AppDir. Open Leo from Start, or type leo in a new terminal."
    Write-Host 'Your notes stay where they are. Thank you for using Leo!'
} finally {
    foreach ($leftover in @($linkTemp, $recordTemp)) { if ($leftover) { Remove-Item -LiteralPath $leftover -Force -ErrorAction SilentlyContinue } }
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
