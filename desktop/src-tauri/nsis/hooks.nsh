; Runtime data lives under the bundle id so it cannot collide with
; $INSTDIR ($LOCALAPPDATA\Harbor). Tauri already deletes $LOCALAPPDATA\${BUNDLEID}
; when the user checks "Delete app data"; this hook also drops the kernel
; extract dir. Skip both on /UPDATE (the default upgrade path).
; currentUser installer is medium IL: these nsExec calls cannot kill an
; elevated leftover kernel or remove an admin-created sing-tun. Best-effort
; only. Path filter leaves an unrelated sing-box.exe alone.
; In NSIS strings: $$ = literal $, $" = literal ".

; 运行中的 harbor.exe 是提权进程，currentUser 安装包（中权限）杀不掉它。
; 不退出就装：旧 exe 被 Windows 锁定无法替换，装完启动/任务栏启动的都还是
; 旧进程（单实例 mutex 会让新进程聚焦旧窗口后退出）。所以安装/卸载前
; 必须请用户先从托盘退出。
; 注意：NSIS 卸载段只能 Call 以 un. 开头的函数，且函数内标签不能重名，故两份。
Function HarborEnsureNotRunning
  harbor_enr_install_retry:
    nsExec::Exec "cmd /c tasklist /FI $\"IMAGENAME eq harbor.exe$\" /FO CSV /NH | findstr /I $\"harbor.exe$\""
    Pop $0
    ${If} $0 == 0
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "检测到 Harbor 正在运行。$\r$\n$\r$\n它以管理员权限运行，安装程序无法自动结束。$\r$\n请在托盘图标上右键选择「退出」，然后点击「重试」。" /SD IDCANCEL IDRETRY harbor_enr_install_retry
      Abort "Harbor 正在运行，已中止。请先从托盘退出后再试。"
    ${EndIf}
FunctionEnd

Function un.HarborEnsureNotRunning
  harbor_enr_uninstall_retry:
    nsExec::Exec "cmd /c tasklist /FI $\"IMAGENAME eq harbor.exe$\" /FO CSV /NH | findstr /I $\"harbor.exe$\""
    Pop $0
    ${If} $0 == 0
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "检测到 Harbor 正在运行。$\r$\n$\r$\n它以管理员权限运行，卸载程序无法自动结束。$\r$\n请在托盘图标上右键选择「退出」，然后点击「重试」。" /SD IDCANCEL IDRETRY harbor_enr_uninstall_retry
      Abort "Harbor 正在运行，已中止。请先从托盘退出后再试。"
    ${EndIf}
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
  Call HarborEnsureNotRunning
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Call un.HarborEnsureNotRunning
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec "powershell -NoProfile -NonInteractive -WindowStyle Hidden -Command $\"Get-Process sing-box -ErrorAction SilentlyContinue | Where-Object { $$_.Path -like '$TEMP\harbor-sing-box\*' } | Stop-Process -Force$\""
  Pop $0
  nsExec::Exec "powershell -NoProfile -NonInteractive -WindowStyle Hidden -Command $\"Get-NetAdapter -Name 'sing-tun' -ErrorAction SilentlyContinue | Remove-NetAdapter -Confirm:$$false$\""
  Pop $0
  ; 开机自启的任务计划（若用户在托盘勾选过）。同为 best-effort。
  nsExec::Exec "schtasks /Delete /TN Harbor /F"
  Pop $0
  ${If} $UpdateMode <> 1
  ${AndIf} $DeleteAppDataCheckboxState = 1
    RMDir /r "$LOCALAPPDATA\io.github.yuxiangluo.harbor"
    RMDir /r "$TEMP\harbor-sing-box"
  ${EndIf}
!macroend
