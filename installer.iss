; Inno Setup script — builds the Windows installer from staging\ (okra exe +
; ffmpeg.exe + licenses). Version is passed by CI: iscc /DAppVersion=v0.2.0 installer.iss
[Setup]
AppName=okra-clip-archiver
AppVersion={#AppVersion}
AppPublisher=nikkibreanne
DefaultDirName={autopf}\okra-clip-archiver
DisableProgramGroupPage=yes
OutputDir=dist
OutputBaseFilename=okra-clip-archiver-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64
WizardStyle=modern

[Files]
Source: "staging\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{autoprograms}\okra-clip-archiver"; Filename: "{app}\okra-clip-archiver.exe"; Parameters: "serve"; WorkingDir: "{app}"
Name: "{autodesktop}\okra-clip-archiver"; Filename: "{app}\okra-clip-archiver.exe"; Parameters: "serve"; WorkingDir: "{app}"

[Run]
Filename: "{app}\okra-clip-archiver.exe"; Parameters: "serve"; Description: "Launch okra-clip-archiver"; Flags: postinstall nowait skipifsilent
