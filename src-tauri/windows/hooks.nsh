; Nullex NSIS installer hooks
; Kills running processes before install/uninstall so files can be overwritten

!macro NSIS_HOOK_PREINSTALL
  ; Kill Nullex.exe and nullex-tauri.exe if running
  nsExec::Exec 'taskkill /F /IM "Nullex.exe" /T'
  nsExec::Exec 'taskkill /F /IM "nullex-tauri.exe" /T'
  nsExec::Exec 'taskkill /F /IM "nullex-run.exe" /T'
  ; Kill mihomo.exe processes (only non-service, i.e. spawned by Nullex)
  nsExec::Exec 'taskkill /F /IM "mihomo.exe" /T'
  ; Brief pause so the OS releases file handles
  Sleep 1500
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'taskkill /F /IM "Nullex.exe" /T'
  nsExec::Exec 'taskkill /F /IM "nullex-tauri.exe" /T'
  nsExec::Exec 'taskkill /F /IM "nullex-run.exe" /T'
  nsExec::Exec 'taskkill /F /IM "mihomo.exe" /T'
  Sleep 1500
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
