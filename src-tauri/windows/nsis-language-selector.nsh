; 即使 NSIS 已在注册表中保存过上次选择的安装语言，也必须再次显示语言选择框，避免用户以为安装包没有语言选择能力。
; Always show the NSIS language selector even when a previous installer language was stored in the registry, so users can clearly switch the installer UI language.
!define MUI_LANGDLL_ALWAYSSHOW

; Unicode 安装器仍显式允许列出所有语言，避免系统区域或代码页判断把简体中文从选择框中过滤掉。
; Explicitly keep all listed languages visible in the Unicode installer, preventing locale or code-page checks from hiding Simplified Chinese.
!define MUI_LANGDLL_ALLLANGUAGES
