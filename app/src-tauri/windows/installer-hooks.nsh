; The stock Tauri uninstaller owns its unchecked Delete App Data option.
; Updating never deletes app data. Do not add broad filesystem cleanup here.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} $DeleteAppDataCheckboxState = 1
    MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "删除栖点数据会移除实例配置、会话、恢复点和引擎缓存，无法撤销。外部项目文件不会删除。Windows 凭据管理器中的 Perch/model 条目会保留，可在那里手动移除。继续删除应用数据？" /SD IDNO IDYES perch_delete_confirmed
      StrCpy $DeleteAppDataCheckboxState 0
    perch_delete_confirmed:
  ${EndIf}
!macroend
