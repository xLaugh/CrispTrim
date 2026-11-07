// Ressources Windows du programme, reprises de CrispTrim.exe (ressources RT_GROUP_ICON 1, RT_VERSION 1,
// RT_MANIFEST 1) : logo CrispTrim en 9 tailles (16 à 256 px), informations de version et manifeste.
fn main() {
    println!("cargo:rerun-if-changed=assets/crisptrim.ico");
    // Version carte graphique : les bibliothèques NVIDIA ne sont chargées qu'au premier appel CUDA
    // (chargement différé), après vérification qu'elles existent. Sans elles, le programme démarre
    // quand même et transcrit sur le processeur.
    if std::env::var_os("CARGO_FEATURE_CUDA").is_some() && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for dll in ["nvcuda.dll", "cudart64_12.dll", "cublas64_12.dll", "cublasLt64_12.dll", "curand64_10.dll", "nvrtc64_120_0.dll"] {
            println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
        }
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
    }
    #[cfg(windows)]
    {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/crisptrim.ico");
        res.set("CompanyName", "xLaugh");
        res.set("FileDescription", "CrispTrim");
        res.set("FileVersion", &version);
        res.set("InternalName", "CrispTrim");
        res.set("LegalCopyright", "xLaugh, GNU GPL v3");
        res.set("OriginalFilename", "crisptrim.exe");
        res.set("ProductName", "CrispTrim");
        res.set("ProductVersion", &version);
        // Comme l'original : sans élévation, chemins longs (> 260 caractères), contrôles Windows modernes.
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{e2011457-1546-43c5-a5fe-008deee3d3f0}"/>
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#,
        );
        if let Err(e) = res.compile() {
            // Sans compilateur de ressources, le programme garde l'icône de sa fenêtre seulement.
            println!("cargo:warning=ressources du .exe non intégrées : {e}");
        }
    }
}
