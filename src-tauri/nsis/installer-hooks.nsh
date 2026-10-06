; Installer hooks for Handy: add handy.exe to the user PATH.
;
; Wired via bundle.windows.nsis.installerHooks in tauri.conf.json and included
; by our custom NSIS template (src-tauri/nsis/installer.nsi) through its
; {{installer_hooks}} placeholder. Tauri only renders that placeholder when
; installer_hooks is set, so this file must always define the four macros
; below (see Tauri docs: NSIS Installer Hooks).
;
; We use the EnVar plugin (bundled with the NSIS toolchain Tauri downloads)
; so PATH entries are handled idempotently and removed cleanly on uninstall.
; PATH is edited at user scope (HKCU\Environment), which works for both
; per-user installs and per-machine installs done by an elevated user.

!macro NSIS_HOOK_POSTINSTALL
  ; --- PORTABLE MODE --- skip PATH changes for portable installs
  ${If} $PortableMode = 0
    DetailPrint "Adding ${MAINBINARYNAME}.exe directory to user PATH"
    EnVar::SetHKCU
    EnVar::Check "Path" "$INSTDIR"
    Pop $0
    ${If} $0 <> 0
      EnVar::AddValue "Path" "$INSTDIR"
      Pop $0
    ${EndIf}
    SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DetailPrint "Removing ${MAINBINARYNAME}.exe directory from user PATH"
  EnVar::SetHKCU
  EnVar::DeleteValue "Path" "$INSTDIR"
  Pop $0
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
