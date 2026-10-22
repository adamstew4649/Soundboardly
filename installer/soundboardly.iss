; Built by CI: iscc /DAppVersion=x.y.z installer\soundboardly.iss
; Expects dist\ to contain soundboardly.exe, yt-dlp.exe and ffmpeg.exe.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F1B8E52-3D1C-4C6B-9B0E-5A0F7C2B9A41}
AppName=Soundboardly
AppVersion={#AppVersion}
AppPublisher=Soundboardly
DefaultDirName={autopf}\Soundboardly
DefaultGroupName=Soundboardly
UninstallDisplayIcon={app}\soundboardly.exe
OutputDir=..\dist-installer
OutputBaseFilename=Soundboardly-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; Flags: unchecked

[Files]
Source: "..\dist\soundboardly.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\yt-dlp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\ffmpeg.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{group}\Soundboardly"; Filename: "{app}\soundboardly.exe"
Name: "{group}\Uninstall Soundboardly"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Soundboardly"; Filename: "{app}\soundboardly.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\soundboardly.exe"; Description: "Launch Soundboardly"; Flags: nowait postinstall skipifsilent
Filename: "https://vb-audio.com/Cable/"; Description: "Open the VB-Cable download page (needed to send clips to your mic)"; Flags: shellexec nowait postinstall skipifsilent unchecked
