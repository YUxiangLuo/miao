; Runtime data lives under the bundle id so it cannot collide with
; $INSTDIR ($LOCALAPPDATA\Miao). Tauri already deletes $LOCALAPPDATA\${BUNDLEID}
; when the user checks "Delete app data"; this hook also drops the kernel
; extract dir. Skip both on /UPDATE (the default upgrade path).
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} $DeleteAppDataCheckboxState = 1
    RMDir /r "$LOCALAPPDATA\io.github.yuxiangluo.miao"
    RMDir /r "$TEMP\miao-sing-box"
  ${EndIf}
!macroend
