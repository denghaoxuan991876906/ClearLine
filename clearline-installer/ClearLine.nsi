Unicode True
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "nsDialogs.nsh"

!ifndef BACKEND_EXE
  !error "BACKEND_EXE must point to clearline-setup.exe"
!endif
!ifndef OUTPUT_EXE
  !define OUTPUT_EXE "ClearLineSetup.exe"
!endif
!ifndef APP_ICON
  !error "APP_ICON must point to clearline.ico"
!endif

Name "ClearLine"
OutFile "${OUTPUT_EXE}"
InstallDir "$PROGRAMFILES64\ClearLine"
InstallDirRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClearLine" "InstallLocation"
BrandingText "ClearLine"
Icon "${APP_ICON}"
ShowInstDetails show

VIProductVersion "0.1.0.0"
VIAddVersionKey /LANG=2052 "ProductName" "ClearLine"
VIAddVersionKey /LANG=2052 "FileDescription" "ClearLine Installer"
VIAddVersionKey /LANG=2052 "FileVersion" "0.1.0"
VIAddVersionKey /LANG=2052 "LegalCopyright" "ClearLine contributors"

!define MUI_ABORTWARNING
!define MUI_ICON "${APP_ICON}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\ClearLine.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Run ClearLine"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
Page custom StartupPage StartupPageLeave
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Var StartupDialog
Var StartupCheckbox
Var StartOnLogin
Var BackendArgs
Var ExistingStartupCommand

Function .onInit
  SetRegView 64
  StrCpy $StartOnLogin 0
  ReadRegStr $ExistingStartupCommand HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ClearLine"
  ${If} $ExistingStartupCommand != ""
    StrCpy $StartOnLogin 1
  ${EndIf}
FunctionEnd

Function StartupPage
  nsDialogs::Create 1018
  Pop $StartupDialog
  ${If} $StartupDialog == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 28u "${U+9009}${U+62E9} ClearLine ${U+7684}${U+542F}${U+52A8}${U+65B9}${U+5F0F}${U+3002}${U+7A0D}${U+540E}${U+53EF}${U+5728}${U+5E94}${U+7528}${U+4E2D}${U+66F4}${U+6539}${U+6B64}${U+8BBE}${U+7F6E}${U+3002}"
  Pop $0
  ${NSD_CreateCheckbox} 0 38u 100% 14u "${U+767B}${U+5F55} Windows ${U+65F6}${U+81EA}${U+52A8}${U+542F}${U+52A8} ClearLine"
  Pop $StartupCheckbox
  ${If} $StartOnLogin == 1
    ${NSD_Check} $StartupCheckbox
  ${EndIf}

  nsDialogs::Show
FunctionEnd

Function StartupPageLeave
  ${NSD_GetState} $StartupCheckbox $StartOnLogin
FunctionEnd

Section "ClearLine" SEC_MAIN
  SectionIn RO
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "/oname=ClearLineSetupBackend.exe" "${BACKEND_EXE}"

  ${If} $StartOnLogin == ${BST_CHECKED}
    StrCpy $BackendArgs "--start-on-login"
  ${Else}
    StrCpy $BackendArgs "--no-start-on-login"
  ${EndIf}

  DetailPrint "${U+6B63}${U+5728}${U+5B89}${U+88C5} ClearLine ${U+548C}${U+865A}${U+62DF}${U+97F3}${U+9891}${U+7EC4}${U+4EF6}..."
  ExecWait '"$PLUGINSDIR\ClearLineSetupBackend.exe" --install --quiet --target "$INSTDIR" $BackendArgs' $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "ClearLine ${U+5B89}${U+88C5}${U+5931}${U+8D25}${U+FF0C}${U+9519}${U+8BEF}${U+4EE3}${U+7801}${U+FF1A}$0${U+3002}${U+8BF7}${U+67E5}${U+770B} ProgramData\ClearLine\logs ${U+4E0B}${U+7684}${U+5B89}${U+88C5}${U+65E5}${U+5FD7}${U+3002}"
    SetErrorLevel $0
    Abort
  ${EndIf}
SectionEnd
