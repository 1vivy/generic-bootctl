#!/system/bin/sh
MODDIR=${0%/*}
[ ! -e "$MODDIR/disable" ] && [ ! -e "$MODDIR/remove" ] || exit 0
while IFS= read -r SVC; do
  case "$SVC" in ""|*[!a-zA-Z0-9_.-]*) exit 1 ;; esac
  if [ "$(getprop "init.svc.$SVC")" = running ]; then
    stop "$SVC" && start "$SVC" || exit 1
  fi
done < "$MODDIR/svc.name"
