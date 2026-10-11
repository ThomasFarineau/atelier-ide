; Windows installer of Atelier IDE (Inno Setup 6 or 7), built by `.github/workflows/build.yml`:
;
;   iscc /DAppVersion=0.1.0 /DSourceDir=dist\stage /DOutputDir=dist installer\atelier.iss
;
; It installs for the current user only, without administrator rights, in the folder the bootstrap
; updates by itself (src/bin/atelier.rs): `atelier.exe` (the bootstrap) and `app\` (the IDE and its
; VERSION). The shortcuts start the bootstrap, which updates the IDE before running it. `bin\`, on the
; PATH, holds the `atelier` command for cmd, PowerShell and Git Bash.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\stage"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

[Setup]
AppId={{6F3C1B52-8D2E-4C7A-9B41-3E5A7D0C9F18}
AppName=Atelier IDE
AppVersion={#AppVersion}
AppVerName=Atelier IDE {#AppVersion}
AppPublisher=Thomas Farineau
AppPublisherURL=https://github.com/ThomasFarineau/atelier-ide
DefaultDirName={localappdata}\Programs\Atelier
DefaultGroupName=Atelier IDE
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
OutputDir={#OutputDir}
OutputBaseFilename=atelier-setup-windows-x86_64
SetupIconFile=..\assets\logo\atelier.ico
UninstallDisplayIcon={app}\atelier.exe
UninstallDisplayName=Atelier IDE
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; The IDE must be closed to be replaced.
CloseApplications=yes

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
fr.AddToPath=Ajouter la commande atelier au PATH
en.AddToPath=Add the atelier command to the PATH
fr.LaunchAtelier=Lancer Atelier IDE
en.LaunchAtelier=Start Atelier IDE

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "addtopath"; Description: "{cm:AddToPath}"

[Files]
Source: "{#SourceDir}\atelier.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\atelier-ide.exe"; DestDir: "{app}\app"; Flags: ignoreversion
Source: "{#SourceDir}\VERSION"; DestDir: "{app}\app"; Flags: ignoreversion
; The atelier command, like bin\code.cmd of VS Code: only this folder goes on the PATH.
Source: "atelier.cmd"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "atelier"; DestDir: "{app}\bin"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Atelier IDE"; Filename: "{app}\atelier.exe"
Name: "{autodesktop}\Atelier IDE"; Filename: "{app}\atelier.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}\bin"; Tasks: addtopath; Check: NotOnPath(ExpandConstant('{app}\bin'))

[Run]
Filename: "{app}\atelier.exe"; Description: "{cm:LaunchAtelier}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; What the bootstrap writes next to itself: updates, its log, the time of the last check.
Type: filesandordirs; Name: "{app}\app"
Type: filesandordirs; Name: "{app}\app.new"
Type: filesandordirs; Name: "{app}\app.old"
Type: files; Name: "{app}\atelier.exe.old"
Type: files; Name: "{app}\bootstrap.log"
Type: files; Name: "{app}\last_check"

[Code]
function UserPath(): string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Result) then
    Result := '';
end;

function NotOnPath(Dir: string): Boolean;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(UserPath()) + ';') = 0;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Path, Dir: string;
  P: Integer;
begin
  if CurUninstallStep <> usPostUninstall then
    exit;
  Path := ';' + UserPath() + ';';
  Dir := ';' + ExpandConstant('{app}\bin') + ';';
  P := Pos(Uppercase(Dir), Uppercase(Path));
  if P = 0 then
    exit;
  Delete(Path, P, Length(Dir) - 1);
  Path := Copy(Path, 2, Length(Path) - 2);
  RegWriteExpandStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Path);
end;
