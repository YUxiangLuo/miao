!macro NSIS_HOOK_POSTUNINSTALL
  RMDir /r "$LOCALAPPDATA\miao"
  RMDir /r "$TEMP\miao-sing-box"
!macroend
