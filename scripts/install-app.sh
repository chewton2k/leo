#!/bin/sh
set -eu
REPO=chewton2k/leo-cli
BIN_DIR=${LEO_INSTALL_DIR:-$HOME/.local/bin}
APPLICATIONS=${LEO_APPLICATIONS_DIR:-/Applications}
fail() { printf 'Leo could not be installed: %s\n' "$*" >&2; exit 1; }
case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target=aarch64-apple-darwin ;;
    Darwin-x86_64) target=x86_64-apple-darwin ;;
    Linux-*) fail 'Leo is available for macOS and Windows only. Nothing was changed.' ;;
    *) fail 'Use install.ps1 in PowerShell on Windows, or download Leo for macOS.' ;;
esac
if printf '%s' "$BIN_DIR$APPLICATIONS" | LC_ALL=C grep -q '[[:cntrl:]]'; then fail 'Choose installation folders without control characters.'; fi
case "$BIN_DIR" in /*) ;; *) fail 'LEO_INSTALL_DIR must be an absolute folder.' ;; esac
case "$APPLICATIONS" in /*) ;; *) fail 'The applications folder must be absolute.' ;; esac
app="$APPLICATIONS/Leo.app"
running() { ps -axo comm= 2>/dev/null | LC_ALL=C grep -Fxq "$app/Contents/MacOS/leo-app"; }
if running; then fail 'Quit Leo before installing. Finish any recording or dictation first.'; fi
work=$(mktemp -d)
staged=''
parked=''
link=''
record_tmp=''
link_done=''
record_done=''
ok=''
config=${LEO_HOME:-$HOME/Library/Application Support/leo}
cleanup() {
    status=$?
    trap - EXIT
    set +e
    [ -z "$link" ] || rm -f "$link"
    [ -z "$record_tmp" ] || rm -f "$record_tmp"
    if [ -z "$ok" ]; then
        if [ -n "$parked" ] && [ -d "$parked" ] && [ ! -e "$app" ]; then mv "$parked" "$app"; fi
        if [ -n "$link_done" ]; then
            if [ -n "$old_link" ]; then rm -f "$BIN_DIR/leo"; ln -s "$old_link" "$BIN_DIR/leo"
            elif [ -f "$work/old-leo" ]; then cp -p "$work/old-leo" "$BIN_DIR/leo"
            else rm -f "$BIN_DIR/leo"; fi
        fi
        if [ -n "$record_done" ]; then
            if [ -f "$work/old-record" ]; then cp "$work/old-record" "$config/app-cli-dir"; else rm -f "$config/app-cli-dir"; fi
        fi
        if [ -n "$had_app" ]; then printf 'The previous installation was left as it was.\n' >&2; else printf 'Nothing was installed.\n' >&2; fi
    fi
    if [ -z "$ok" ] && [ -n "$parked" ] && [ -d "$parked" ]; then
        printf 'The previous app is still in %s.\n' "$staged" >&2
    elif [ -n "$staged" ]; then rm -rf "$staged"; fi
    rm -rf "$work"
    exit "$status"
}
old_link=''
had_app=''
trap cleanup EXIT
trap 'exit 130' INT TERM
fetch() { curl --fail --location --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 900 --max-filesize 1073741824 --retry 2 --output "$2" "$1"; }
archive="$work/Leo.app.tar.gz"
if [ -n "${LEO_INSTALL_ARCHIVE:-}" ]; then
    cp "$LEO_INSTALL_ARCHIVE" "$archive"
else
    tag=$(curl --fail --silent --location --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 30 --output /dev/null --write-out '%{url_effective}' "https://github.com/$REPO/releases/latest") || fail 'GitHub did not answer. Check your connection and try again.'
    tag=${tag##*/}
    case "$tag" in v[0-9]*.[0-9]*.[0-9]*) ;; *) fail 'The latest release could not be identified. Try again later.' ;; esac
    url="https://github.com/$REPO/releases/download/$tag/leo-$target.app.tar.gz"
    fetch "$url" "$archive" || fail 'The app download failed. Check your connection and try again.'
    fetch "$url.sha256" "$work/checksum" || fail 'The release checksum is missing. Try again after the release is repaired.'
    expected=$(awk 'NR==1 {print $1}' "$work/checksum")
    actual=$(shasum -a 256 "$archive" | awk '{print $1}')
    [ "$expected" = "$actual" ] || fail 'The app download is damaged. Try downloading it again.'
fi
[ "$(wc -c < "$archive" | tr -d ' ')" -le 1073741824 ] || fail 'The app archive is too large.'
tar -tzf "$archive" | LC_ALL=C awk 'NR>10000 || length($0)>2048 || $0 !~ /^Leo[.]app(\/|$)/ || $0 ~ /(^|\/)\.\.?(\/|$)/ || $0 ~ /[[:cntrl:]]/ {bad=1;exit 1} END {exit (bad || NR==0)}' || fail 'The archive contains unsafe paths or too many files.'
tar -tvzf "$archive" | LC_ALL=C awk 'NR>10000 || $0 !~ /^[-d]/ {bad=1;exit 1} {total+=$5; if(total>4294967296) {bad=1;exit 1}} END {exit (bad || NR==0)}' || fail 'The archive contains links or too much unpacked data.'
mkdir "$work/unpacked"
tar -xzf "$archive" -C "$work/unpacked" || fail 'The app could not be unpacked.'
new="$work/unpacked/Leo.app"
[ -f "$new/Contents/MacOS/leo" ] && [ -x "$new/Contents/MacOS/leo-app" ] || fail 'The archive is missing the Leo app or terminal command.'
identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$new/Contents/Info.plist" 2>/dev/null) || fail 'The app information could not be read.'
[ "$identifier" = io.github.chewton2k.leo ] || fail 'This archive is not the Leo app.'
codesign --verify --deep --strict "$new" || fail 'The app signature is damaged. Download it again.'
if running; then fail 'Quit Leo before installing. Finish any recording or dictation first.'; fi
[ ! -L "$app" ] || fail 'Leo.app is a link. Remove that link before installing.'
if [ -e "$app" ]; then
    old_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist" 2>/dev/null) || fail 'An existing Leo.app could not be identified. Move it aside first.'
    [ "$old_id" = io.github.chewton2k.leo ] || fail 'The existing Leo.app belongs to another application. Move it aside first.'
fi
mkdir -p "$APPLICATIONS" "$BIN_DIR" || fail 'The installation folder is not writable. Install the disk image from Downloads instead.'
mkdir -p "$config" || fail 'The Leo settings folder could not be created.'
[ ! -L "$config/app-cli-dir" ] || fail 'The saved terminal folder is a link. Remove app-cli-dir from the Leo settings folder.'
[ ! -e "$app" ] || had_app=1
if [ -L "$BIN_DIR/leo" ]; then old_link=$(readlink "$BIN_DIR/leo")
elif [ -f "$BIN_DIR/leo" ]; then cp -p "$BIN_DIR/leo" "$work/old-leo"; fi
if [ -f "$config/app-cli-dir" ]; then cp -p "$config/app-cli-dir" "$work/old-record"; fi
staged=$(mktemp -d "$APPLICATIONS/.leo-install.XXXXXX")
ditto "$new" "$staged/Leo.app" || fail 'The app could not be copied. Check free disk space.'
codesign --verify --deep --strict "$staged/Leo.app" || fail 'The copied app failed its signature check.'
link="$BIN_DIR/.leo-link.$$"
ln -s "$app/Contents/MacOS/leo" "$link" || fail 'The terminal command could not be prepared. Check that its folder is writable.'
record_tmp=$(mktemp "$config/.app-cli-dir.XXXXXX") || fail 'The terminal command could not be prepared. Check that its folder is writable.'
chmod 600 "$record_tmp"
printf '%s\n' "$BIN_DIR" > "$record_tmp" || fail 'The terminal command could not be prepared. Check that its folder is writable.'
mv -f "$link" "$BIN_DIR/leo" || fail 'The terminal command could not be put in place.'
link_done=1
link=''
mv -f "$record_tmp" "$config/app-cli-dir" || fail 'The terminal folder could not be saved.'
record_done=1
record_tmp=''
if [ -e "$app" ]; then parked="$staged/previous.app"; mv "$app" "$parked" || fail 'The previous app could not be moved aside.'; fi
mv "$staged/Leo.app" "$app" || fail 'The app could not be put in Applications.'
ok=1
profile_line() {
    [ -z "${LEO_INSTALL_SKIP_PATH:-}" ] || return 0
    quoted=$(printf '%s' "$BIN_DIR" | sed "s/'/'\\\\''/g") || return 1
    line="export PATH='$quoted':\"\$PATH\""
    shell_name=${SHELL:-sh}
    case "${shell_name##*/}" in
        zsh) rc="$HOME/.zshrc" ;;
        bash) rc="$HOME/.bash_profile" ;;
        fish) rc="$HOME/.config/fish/config.fish"; line="fish_add_path -- '$quoted'" ;;
        *) rc="$HOME/.profile" ;;
    esac
    if [ -L "$rc" ]; then
        printf 'Your shell profile is a link. Add %s to PATH there yourself.\n' "$BIN_DIR"
    elif ! grep -qsFx "$line" "$rc"; then
        mkdir -p "$(dirname "$rc")" &&
        rc_tmp=$(mktemp "$(dirname "$rc")/.leo-path.XXXXXX") &&
        { [ ! -f "$rc" ] || cp -p "$rc" "$rc_tmp"; } &&
        printf '\n%s\n%s\n' '# Added by the leo installer' "$line" >> "$rc_tmp" &&
        mv -f "$rc_tmp" "$rc" || { [ -z "${rc_tmp:-}" ] || rm -f "$rc_tmp"; return 1; }
    fi
}
profile_line || printf 'leo could not add %s to your PATH. Add it to your shell profile yourself.\n' "$BIN_DIR"
printf '\nLeo is installed in %s.\nTerminal command: %s/leo\nOpen Leo in Applications, or type leo. Open a new terminal if needed.\nYour notes stay where they are. Thank you for using Leo!\n' "$app" "$BIN_DIR"
