#!/system/bin/sh
# KernelSU sources this file; no partitions or properties are modified here.
SKIPUNZIP=0
ROOT=${GENERIC_BOOTCTL_TEST_ROOT:-/}
fail() { abort "generic_bootctl: $*"; exit 1; }
[ "${KSU:-}" = true ] || [ "${KSU:-}" = 1 ] || fail "KernelSU required"
[ "$(getprop persist.generic_bootctl.rw)" != 1 ] || fail "disable persist.generic_bootctl.rw before installation"
case "$(getprop ro.product.cpu.abi)" in
  arm64-v8a) ARCH=aarch64 ;;
  x86_64) ARCH=x86_64 ;;
  *) fail "unsupported architecture (64-bit required)" ;;
esac
[ -x "$MODPATH/impl/$ARCH/bootctl-unified" ] || fail "missing architecture payload"
# One installed executable path is also used by the sandboxed vendor probe.
cp "$MODPATH/impl/$ARCH/bootctl-unified" "$MODPATH/impl/bootctl-unified" || fail "payload copy failed"
cp "$MODPATH/impl/$ARCH/bootctrl.module.so" "$MODPATH/impl/bootctrl.module.so" || fail "legacy payload copy failed"
chmod 0755 "$MODPATH/impl/bootctl-unified" || fail "payload chmod failed"
"$MODPATH/impl/bootctl-unified" --install-plan "$ROOT" > "$MODPATH/install.plan" || fail "manifest/rc discovery refused installation"
CUR=$(bootctl get-current-slot) || fail "cannot query stock current slot"
case "$CUR" in 0|1|2|3) ;; *) fail "invalid stock current slot" ;; esac
bootctl is-slot-marked-successful "$CUR" || fail "current slot not marked successful; OTA tries must not be exhausted"
bootctl get-number-slots > "$MODPATH/stock-slots.txt" || fail "cannot capture stock slot count"
: > "$MODPATH/svc.name"
while IFS='|' read -r KIND SVC TARGET; do
  [ -n "$TARGET" ] || fail "empty overlay target"
  SOURCE="$ROOT/${TARGET#/}"
  [ -f "$SOURCE" ] || fail "missing original $TARGET"
  mkdir -p "$MODPATH/backup$(dirname "$TARGET")" || fail "backup directory failed"
  cp -p "$SOURCE" "$MODPATH/backup$TARGET" || fail "backup failed"
  # KernelSU magic mount maps system/vendor etc. onto separate partitions.
  case "$TARGET" in /system/*) DEST="$MODPATH$TARGET" ;; *) DEST="$MODPATH/system$TARGET" ;; esac
  mkdir -p "$(dirname "$DEST")" || fail "overlay directory failed"
  case "$KIND" in
    service) PAYLOAD=bootctl-unified; printf '%s\n' "$SVC" >> "$MODPATH/svc.name" ;;
    legacy) PAYLOAD=bootctrl.module.so ;;
    *) fail "invalid install plan" ;;
  esac
  cp "$MODPATH/impl/$PAYLOAD" "$DEST" || fail "overlay copy failed"
  chmod 0755 "$DEST" || fail "overlay chmod failed"
  chcon --reference="$SOURCE" "$DEST" || fail "cannot inherit stock SELinux label; no policy workaround"
done < "$MODPATH/install.plan"
[ -s "$MODPATH/svc.name" ] || fail "no service discovered"
ui_print "generic_bootctl installed read-only; stock vendor files are unchanged"
