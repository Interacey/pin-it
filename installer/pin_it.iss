; Inno Setup 6 script for Pin It. Build from the repository root:
;   ISCC.exe /DAppVersion=0.1.0 installer\pin_it.iss
; Output: installer\Output\PinIt-Setup.exe (fixed name, no version in it).
; /DExePath=<path> points at a different pin_it.exe than target\release\pin_it.exe.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef ExePath
  #define ExePath "..\target\release\pin_it.exe"
#endif

[Setup]
; AppId identifies the program for upgrades and uninstall. It must never change.
AppId={{E2D3BC67-742E-4865-AA60-368B5173512E}
AppName=Pin It
AppVersion={#AppVersion}
; Windows file version needs four numeric parts.
VersionInfoVersion={#AppVersion}.0
AppPublisher=Interacey
AppPublisherURL=https://github.com/Interacey/pin-it
AppSupportURL=https://github.com/Interacey/pin-it
AppUpdatesURL=https://github.com/Interacey/pin-it
; Per user, no admin rights: {autopf} is %LOCALAPPDATA%\Programs.
PrivilegesRequired=lowest
DefaultDirName={autopf}\Pin It
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\assets\pin_it.ico
UninstallDisplayIcon={app}\pin_it.exe
UninstallDisplayName=Pin It
LicenseFile=..\LICENSE
OutputDir=Output
OutputBaseFilename=PinIt-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Restart Manager is off: Pin It is closed in [Code] with WM_CLOSE, which unpins all windows.
CloseApplications=no

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Pin It"; Filename: "{app}\pin_it.exe"

; No autostart task: Pin It writes its own HKCU Run value "PinIt" from the tray item
; "Start with Windows" and removes a value that does not match its setting at every start.
[Run]
; Upgrade: start again only if it was running when Setup began.
Filename: "{app}\pin_it.exe"; Flags: nowait; Check: WasRunning
; Fresh install: ask on the last page.
Filename: "{app}\pin_it.exe"; Description: "Start Pin It"; Flags: postinstall skipifsilent nowait; Check: not WasRunning

[Code]
const
  MainClass = 'PinItMain';
  WM_CLOSE = $0010;
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';

var
  PinItWasRunning: Boolean;

function WasRunning: Boolean;
begin
  Result := PinItWasRunning;
end;

{ Asks the hidden main window to close and waits up to 5 s. Pin It releases all pins on exit. }
function CloseRunningPinIt: Boolean;
var
  Wnd: HWND;
  I: Integer;
begin
  Wnd := FindWindowByClassName(MainClass);
  Result := Wnd = 0;
  if Result then
    Exit;
  PinItWasRunning := True;
  PostMessage(Wnd, WM_CLOSE, 0, 0);
  for I := 1 to 50 do
  begin
    Sleep(100);
    if FindWindowByClassName(MainClass) = 0 then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not CloseRunningPinIt then
    Result := 'Pin It did not close. Choose Exit in its tray menu and run Setup again.';
end;

function InitializeUninstall: Boolean;
begin
  Result := CloseRunningPinIt;
  if not Result then
    MsgBox('Pin It did not close. Choose Exit in its tray menu and run the uninstaller again.',
      mbError, MB_OK);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Value, DataDir: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    { Remove the autostart value only if it points into the install folder (portable copies keep theirs). }
    if RegQueryStringValue(HKCU, RunKey, 'PinIt', Value) and
       (Pos(Lowercase(ExpandConstant('{app}')), Lowercase(Value)) > 0) then
      RegDeleteValue(HKCU, RunKey, 'PinIt');
  end
  else if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{userappdata}\PinIt');
    if DirExists(DataDir) and not UninstallSilent then
      if MsgBox('Delete Pin It settings and log in ' + DataDir + '?',
           mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(DataDir, True, True, True);
  end;
end;
