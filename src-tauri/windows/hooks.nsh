; Keevoro's own steps in the Windows installer Tauri makes.
;
; Uninstalling: Tauri's uninstaller already removes Keevoro's value under
; HKCU\Software\Microsoft\Windows\CurrentVersion\Run (named after the
; product, as the autostart plugin names it) when it is not updating. The
; value Task Manager keeps beside it, to say whether the person turned it
; off there, is removed here too, so nothing of "Start Keevoro when this
; computer starts" is left behind. An update ($UpdateMode) keeps both, so
; starting with the computer survives it.

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${PRODUCTNAME}"
  ${EndIf}
!macroend
