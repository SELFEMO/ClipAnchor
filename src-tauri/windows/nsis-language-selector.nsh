; 即使 NSIS 已在注册表中保存过上次选择的安装语言，也必须再次显示语言选择框，避免用户以为安装包没有语言选择能力。
; Always show the NSIS language selector even when a previous installer language was stored in the registry, so users can clearly switch the installer UI language.
!define MUI_LANGDLL_ALWAYSSHOW

; Unicode 安装器仍显式允许列出所有语言，避免系统区域或代码页判断把简体中文从选择框中过滤掉。
; Explicitly keep all listed languages visible in the Unicode installer, preventing locale or code-page checks from hiding Simplified Chinese.
!define MUI_LANGDLL_ALLLANGUAGES

; 应用内更新使用 NSIS /S。MUI2 的 MUI_LANGDLL_DISPLAY 已用 ${unless} ${Silent} 包住语言框，
; ALWAYSSHOW 只影响交互安装。下面钩子再判断一次 ${Silent}，避免以后改模板时静默更新弹出语言框卡住隐藏进程。
; In-app updates use NSIS /S. MUI2 already wraps LangDLL in ${unless} ${Silent};
; ALWAYSSHOW only affects interactive installs. The hook below re-checks ${Silent}
; so a future template change cannot block a hidden auto-update on the language dialog.
!macro NSIS_HOOK_PREINSTALL
  ${If} ${Silent}
    ; LangDLL ran in .onInit and was skipped; keep this branch so silent installs stay UI-free.
    Nop
  ${EndIf}
!macroend
