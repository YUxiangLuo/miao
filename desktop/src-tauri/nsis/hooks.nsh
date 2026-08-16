; Runtime data lives under the bundle id so it cannot collide with
; $INSTDIR ($LOCALAPPDATA\Miao). Tauri already deletes $LOCALAPPDATA\${BUNDLEID}
; when the user checks "Delete app data"; this hook also drops the kernel
; extract dir. Skip both on /UPDATE (the default upgrade path).
; currentUser installer is medium IL: these nsExec calls cannot kill an
; elevated leftover kernel or remove an admin-created sing-tun. Best-effort
; only. Path filter leaves an unrelated sing-box.exe alone.
; In NSIS strings: $$ = literal $, $\" = literal ".
!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec "powershell -NoProfile -NonInteractive -WindowStyle Hidden -Command $\"Get-Process sing-box -ErrorAction SilentlyContinue | Where-Object { $$_.Path -like '$TEMP\miao-sing-box\*' } | Stop-Process -Force$\""
  Pop $0
  nsExec::Exec "powershell -NoProfile -NonInteractive -WindowStyle Hidden -Command $\"Get-NetAdapter -Name 'sing-tun' -ErrorAction SilentlyContinue | Remove-NetAdapter -Confirm:$$false$\""
  Pop $0
  ; 开机自启的任务计划（若用户在托盘勾选过）。同为 best-effort。
  nsExec::Exec "schtasks /Delete /TN Miao /F"
  Pop $0
  ${If} $UpdateMode <> 1
  ${AndIf} $DeleteAppDataCheckboxState = 1
    RMDir /r "$LOCALAPPDATA\io.github.yuxiangluo.miao"
    RMDir /r "$TEMP\miao-sing-box"
  ${EndIf}
!macroend
