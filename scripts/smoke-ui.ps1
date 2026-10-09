$ErrorActionPreference = 'Stop'
$atelierRoot = Split-Path -Parent $PSScriptRoot
$atelierExe = Join-Path $atelierRoot 'target\release\atelier-ide.exe'
if (-not (Test-Path -LiteralPath $atelierExe)) {
    throw 'Construire la version release avant le test : cargo build --release --locked'
}
$atelierProcess = Start-Process -FilePath $atelierExe -WorkingDirectory $atelierRoot -WindowStyle Hidden -PassThru
try {
    $atelierDeadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 100
        $atelierProcess.Refresh()
        if ($atelierProcess.HasExited) { throw "L'application a quitté au démarrage (code $($atelierProcess.ExitCode))." }
    } while (($atelierProcess.MainWindowHandle -eq 0 -or $atelierProcess.MainWindowTitle -ne 'Atelier IDE') -and [DateTime]::UtcNow -lt $atelierDeadline)
    if ($atelierProcess.MainWindowHandle -eq 0 -or $atelierProcess.MainWindowTitle -ne 'Atelier IDE') { throw 'La fenêtre Atelier IDE ne s’est pas initialisée.' }
    Write-Output "Fenêtre native initialisée : $($atelierProcess.MainWindowTitle)"
    if (-not $atelierProcess.CloseMainWindow()) { throw 'La demande de fermeture a échoué.' }
    if (-not $atelierProcess.WaitForExit(10000)) { throw 'La fermeture normale a dépassé 10 secondes.' }
    if ($atelierProcess.ExitCode -ne 0) { throw "Sortie avec code $($atelierProcess.ExitCode)" }
    Write-Output 'Test natif réussi : ouverture et fermeture sans erreur.'
} finally {
    $atelierProcess.Refresh()
    if (-not $atelierProcess.HasExited) { Stop-Process -Id $atelierProcess.Id -Force }
    $atelierProcess.Dispose()
}
