; Shift - Windows installer (NSIS 3, per-user, no administrator rights).
;
; makensis -DSHIFT_EXE=<path> -DLICENSE_FILE=<path> -DNOTICES_FILE=<path> \
;          -DREADME_FILE=<path> -DICONS_DIR=<path> -DOUT_FILE=<path> \
;          -DVERSION=0.1.0 -DEST_SIZE_KB=<kb> scripts/windows-installer.nsi
;
; Install scope is deliberately per-user: Shift writes only to HKCU and its own
; folder, which is also what the portable build already does.

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinVer.nsh"
!include "x64.nsh"

!ifndef VERSION
  !error "VERSION is required (for example -DVERSION=0.1.0)"
!endif
!ifndef SHIFT_EXE
  !error "SHIFT_EXE is required"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "Shift-Setup.exe"
!endif
!ifndef EST_SIZE_KB
  !define EST_SIZE_KB "40000"
!endif

!define APP_NAME "Shift"
!define APP_ID "app.nex.shift"
!define APP_VERSION "${VERSION}"
!define PUBLISHER "Shift Contributors"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
!define WEBVIEW_KEY "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

Unicode true
Name "${APP_NAME}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "Software\${APP_NAME}" "InstallDirectory"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define /date BUILD_YEAR "%Y"
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey ProductName "${APP_NAME}"
VIAddVersionKey ProductVersion "${APP_VERSION}"
VIAddVersionKey CompanyName "${PUBLISHER}"
VIAddVersionKey LegalCopyright "Copyright ${BUILD_YEAR} ${PUBLISHER}"
VIAddVersionKey FileDescription "${APP_NAME} installer"
VIAddVersionKey FileVersion "${APP_VERSION}.0"
VIAddVersionKey OriginalFilename "Shift-Setup.exe"

!define MUI_ICON "${ICONS_DIR}\icon.ico"
!define MUI_UNICON "${ICONS_DIR}\icon.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\shift.exe"
!define MUI_FINISHPAGE_RUN_TEXT "$(run_shift)"
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "$(read_me)"
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION show_readme

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

LangString run_shift 1033 "Launch Shift when the installer closes."
LangString run_shift 2052 "安装完成后启动 Shift。"
LangString read_me 1033 "Open the Windows notes."
LangString read_me 2052 "打开 Windows 使用说明。"
LangString close_shift 1033 "${APP_NAME} is running and has to close before it can be updated.$\r$\n$\r$\nClick OK to close it now, or Cancel to leave things as they are."
LangString close_shift 2052 "${APP_NAME} 正在运行，需要先退出才能更新。$\r$\n$\r$\n点击“确定”立即退出，点击“取消”保持现状。"
LangString webview_missing 1033 "${APP_NAME} draws its windows with Microsoft Edge WebView2, which this installer did not find.$\r$\n$\r$\nIf Shift opens a blank window or does not start, install the WebView2 Evergreen Runtime from Microsoft and launch Shift again.$\r$\n$\r$\nhttps://developer.microsoft.com/microsoft-edge/webview2/"
LangString webview_missing 2052 "${APP_NAME} 的界面依赖 Microsoft Edge WebView2，安装程序没有检测到它。$\r$\n$\r$\n如果 Shift 启动后窗口空白或无法打开，请从微软官网安装 WebView2 Evergreen Runtime，然后重新启动 Shift。$\r$\n$\r$\nhttps://developer.microsoft.com/microsoft-edge/webview2/"

Var ReadmePath

Function .onInit
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "${APP_NAME} needs Windows 10 or later."
    Abort
  ${EndIf}

  ${IfNot} ${IsNativeAMD64}
    MessageBox MB_OK|MB_ICONSTOP "This build is for 64-bit Windows (x64)."
    Abort
  ${EndIf}

  ; Shift keeps a tray process alive and Windows refuses to overwrite a mapped
  ; EXE, so ask before closing a conversion the user may be waiting on.
  ${If} ${FileExists} "$INSTDIR\shift.exe"
    FindWindow $0 "" "${APP_NAME}"
    ${If} $0 != 0
      MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "$(close_shift)" IDOK +2
        Abort
    ${EndIf}
  ${EndIf}

  nsExec::Exec 'taskkill /IM shift.exe /F'
  Pop $0
FunctionEnd

Function un.onInit
  nsExec::Exec 'taskkill /IM shift.exe /F'
  Pop $0
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  File "${SHIFT_EXE}"
!ifdef LICENSE_FILE
  File /oname=LICENSE.txt "${LICENSE_FILE}"
!endif
!ifdef NOTICES_FILE
  File /oname=THIRD_PARTY_NOTICES.txt "${NOTICES_FILE}"
!endif
!ifdef README_FILE
  File /oname=README-Windows.txt "${README_FILE}"
!endif

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  StrCpy $ReadmePath "$INSTDIR\README-Windows.txt"

  WriteRegStr HKCU "Software\${APP_NAME}" "InstallDirectory" "$INSTDIR"

  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\shift.exe"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\shift.exe"

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\shift.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" ${EST_SIZE_KB}
SectionEnd

Section "PostInstall"
  ; The runtime is machine-wide and normally already present on Windows 10/11,
  ; so a missing one is a warning rather than a reason to refuse the install.
  ClearErrors
  ReadRegStr $0 HKLM "${WEBVIEW_KEY}" "pv"
  ${If} $0 == ""
    ReadRegStr $0 HKCU "${WEBVIEW_KEY}" "pv"
  ${EndIf}
  ${If} $0 == ""
    MessageBox MB_OK|MB_ICONINFORMATION "$(webview_missing)"
  ${EndIf}
SectionEnd

Function show_readme
  ${If} ${FileExists} "$ReadmePath"
    Exec 'notepad.exe "$ReadmePath"'
  ${EndIf}
FunctionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /IM shift.exe /F'
  Pop $0

  Delete "$INSTDIR\shift.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.txt"
  Delete "$INSTDIR\README-Windows.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Uninstall.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  ; Only the keys the installer owns. User preferences and the login item are
  ; managed by Shift itself and stay untouched.
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegValue HKCU "Software\${APP_NAME}" "InstallDirectory"
  DeleteRegKey /ifempty HKCU "Software\${APP_NAME}"
SectionEnd
