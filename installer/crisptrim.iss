; Installateur de CrispTrim (Inno Setup 6) : installation pour l'utilisateur, sans droits administrateur.
;   iscc /DAppVersion=0.3.1 /DExe=dist\CrispTrim.exe installer\crisptrim.iss   →   dist\CrispTrim-Setup.exe
; Mise à jour en un clic : l'appli lance  CrispTrim-Setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
; (elle se ferme avant ; l'installateur la relance à la fin).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Exe
  #define Exe "..\dist\CrispTrim.exe"
#endif

[Setup]
AppId={{6E0B8C9D-4A7E-4C5B-9D61-C215F3A0B7E2}
AppName=CrispTrim
AppVersion={#AppVersion}
AppVerName=CrispTrim {#AppVersion}
AppPublisher=xLaugh
AppPublisherURL=https://github.com/xLaugh/CrispTrim
AppSupportURL=https://github.com/xLaugh/CrispTrim/issues
AppUpdatesURL=https://github.com/xLaugh/CrispTrim/releases
VersionInfoVersion={#AppVersion}
VersionInfoCompany=xLaugh
VersionInfoDescription=Installation de CrispTrim
DefaultDirName={localappdata}\Programs\CrispTrim
DisableDirPage=auto
DisableProgramGroupPage=yes
DefaultGroupName=CrispTrim
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=CrispTrim-Setup
SetupIconFile=..\assets\crisptrim.ico
UninstallDisplayIcon={app}\CrispTrim.exe
UninstallDisplayName=CrispTrim
LicenseFile=..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "desktopicon"; Description: "Créer un raccourci sur le Bureau"; GroupDescription: "Raccourcis :"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "CrispTrim.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\CrispTrim"; Filename: "{app}\CrispTrim.exe"; Comment: "Coupe les silences et les euh de tes rushes"
Name: "{autodesktop}\CrispTrim"; Filename: "{app}\CrispTrim.exe"; Tasks: desktopicon

[Run]
; installation normale : case « Lancer CrispTrim » ; mise à jour silencieuse : relance directe
Filename: "{app}\CrispTrim.exe"; Description: "Lancer CrispTrim"; Flags: nowait postinstall skipifsilent
Filename: "{app}\CrispTrim.exe"; Flags: nowait skipifnotsilent

[Code]
// Avant de désinstaller : fermer CrispTrim et son module carte graphique (sinon leurs fichiers restent).
function InitializeUninstall(): Boolean;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM CrispTrim.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM crisptrim-gpu-*', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(500);
  Result := True;
end;

// À la désinstallation : proposer de supprimer aussi les modèles, le cache et le module carte graphique.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data: String;
begin
  if (CurUninstallStep = usPostUninstall) and (not UninstallSilent) then
  begin
    Data := ExpandConstant('{localappdata}\CrispTrim');
    if DirExists(Data) then
      if MsgBox('Supprimer aussi les modèles de transcription, le cache et le module carte graphique ?' + #13#10 +
                '(' + Data + ', plusieurs Go)' + #13#10#13#10 +
                'Tes vidéos et tes exports ne sont pas touchés.', mbConfirmation, MB_YESNO) = IDYES then
        DelTree(Data, True, True, True);
  end;
end;
