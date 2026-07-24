; Mine Mail's Windows installer theme.
;
; Tauri includes this file from its upstream NSIS template before the Modern UI
; pages are declared. Keeping the customization in a hook file means upgrades,
; uninstall behavior, WebView2 checks, shortcuts and silent installation remain
; owned by Tauri's maintained template.

Caption "安装 Mine Mail"
UninstallCaption "卸载 Mine Mail"
; The approved title bar contains text only. The branded fox remains in the
; installer artwork and executable icon, but is not repeated in the caption.
WindowIcon Off
XPStyle On

; Shared warm frosted palette from the approved installer direction.
!define MUI_BGCOLOR "FFF9F3"
!define MUI_TEXTCOLOR "102A46"
!define MUI_DIRECTORYPAGE_BGCOLOR "FFF9F3"
!define MUI_DIRECTORYPAGE_TEXTCOLOR "102A46"
!define MUI_INSTFILESPAGE_COLORS "102A46 FFF9F3"
!define MUI_HEADERIMAGE_BITMAP_STRETCH "FitControl"
!define MUI_HEADERIMAGE_UNBITMAP_STRETCH "FitControl"
!define MUI_ABORTWARNING

; Welcome state.
!define MUI_WELCOMEPAGE_TITLE "安装 Mine Mail"
!define MUI_WELCOMEPAGE_TITLE_3LINES
!define MUI_WELCOMEPAGE_TEXT "一封来自未来的信$\r$\n$\r$\n安全、专注的桌面邮件客户端。$\r$\n$\r$\n点击“下一步”选择安装位置并继续。"

; Install location state.
!define MUI_DIRECTORYPAGE_TEXT_TOP "选择 Mine Mail 的安装位置。默认位置适合大多数用户。"
!define MUI_DIRECTORYPAGE_TEXT_DESTINATION "安装位置"

; Progress state.
!define MUI_INSTFILESPAGE_FINISHHEADER_TEXT "Mine Mail 已安装"
!define MUI_INSTFILESPAGE_FINISHHEADER_SUBTEXT "文件与快捷方式已经准备就绪。"
!define MUI_INSTFILESPAGE_ABORTHEADER_TEXT "安装未完成"
!define MUI_INSTFILESPAGE_ABORTHEADER_SUBTEXT "你可以稍后重新运行安装程序。"

; Completion state.
!define MUI_FINISHPAGE_TITLE "安装完成"
!define MUI_FINISHPAGE_TITLE_3LINES
!define MUI_FINISHPAGE_TEXT "Mine Mail 已经准备好了。$\r$\n$\r$\n愿每一封重要邮件，都被温柔抵达。"
!define MUI_FINISHPAGE_BUTTON "完成"
!define MUI_FINISHPAGE_RUN_TEXT "安装完成后打开 Mine Mail"
!define MUI_FINISHPAGE_LINK_COLOR "1677FF"

; The welcome page is the branded entry point, so give its native text controls
; the same rounded Windows font used by the persistent buttons.
!define MUI_PAGE_CUSTOMFUNCTION_SHOW MineMailWelcomeShow

Function MineMailWelcomeShow
  FindWindow $R6 "#32770" "" $HWNDPARENT

  GetDlgItem $R7 $HWNDPARENT 1
  SendMessage $R7 ${WM_SETTEXT} 0 "STR:开始安装"

  GetDlgItem $R7 $R6 1201
  CreateFont $R8 "Microsoft YaHei UI" 12 700
  SendMessage $R7 ${WM_SETFONT} $R8 1

  GetDlgItem $R7 $R6 1202
  CreateFont $R8 "Microsoft YaHei UI" 9 400
  SendMessage $R7 ${WM_SETFONT} $R8 1
FunctionEnd

; Apply a friendly system font to the persistent native controls. Page content
; continues to use native NSIS controls for keyboard access and screen readers.
!define MUI_CUSTOMFUNCTION_GUIINIT MineMailGuiInit

Function MineMailGuiInit
  ; MUI applies the executable icon while creating the native window. Clear the
  ; window and class icons after that initialization so the caption stays
  ; text-only while the setup.exe file itself keeps its branded icon.
  InitPluginsDir
  File "/oname=$PLUGINSDIR\mine-mail-blank-window.ico" "${__FILEDIR__}\assets\blank-window.ico"
  System::Call "user32::LoadImageW(p 0, w '$PLUGINSDIR\mine-mail-blank-window.ico', i 1, i 16, i 16, i 0x10) p .r9"
  SendMessage $HWNDPARENT ${WM_SETICON} 0 $R9
  SendMessage $HWNDPARENT ${WM_SETICON} 1 $R9
  ; NSIS itself is a 32-bit process, including for x64 application bundles.
  ; Use the 32-bit WindowLong APIs so this works on every supported Windows.
  System::Call "user32::SetClassLongW(p $HWNDPARENT, i -14, i r9)"
  System::Call "user32::SetClassLongW(p $HWNDPARENT, i -34, i r9)"
  System::Call "user32::GetWindowLongW(p $HWNDPARENT, i -20) i .r7"
  IntOp $R7 $R7 | 0x00000001
  System::Call "user32::SetWindowLongW(p $HWNDPARENT, i -20, i r7)"
  System::Call "user32::SetWindowPos(p $HWNDPARENT, p 0, i 0, i 0, i 0, i 0, i 0x37)"

  CreateFont $R8 "Microsoft YaHei UI" 9 400

  GetDlgItem $R9 $HWNDPARENT 1
  SendMessage $R9 ${WM_SETFONT} $R8 1

  GetDlgItem $R9 $HWNDPARENT 2
  SendMessage $R9 ${WM_SETFONT} $R8 1

  GetDlgItem $R9 $HWNDPARENT 3
  SendMessage $R9 ${WM_SETFONT} $R8 1

  GetDlgItem $R9 $HWNDPARENT 1028
  SendMessage $R9 ${WM_SETFONT} $R8 1
  SetCtlColors $R9 "6F7F90" "FFF9F3"
FunctionEnd
