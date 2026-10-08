$ErrorActionPreference = 'Stop'
$forgeRoot = Split-Path -Parent $PSScriptRoot
$forgeExe = Join-Path $forgeRoot 'target\release\forge-ide.exe'
if (-not (Test-Path -LiteralPath $forgeExe)) {
    throw 'Construire la version release avant le test : cargo build --release --locked'
}
$forgeProcess = Start-Process -FilePath $forgeExe -WorkingDirectory $forgeRoot -WindowStyle Hidden -PassThru
try {
    $forgeDeadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 100
        $forgeProcess.Refresh()
        if ($forgeProcess.HasExited) { throw "L'application a quitté au démarrage (code $($forgeProcess.ExitCode))." }
    } while (($forgeProcess.MainWindowHandle -eq 0 -or $forgeProcess.MainWindowTitle -ne 'Forge IDE') -and [DateTime]::UtcNow -lt $forgeDeadline)
    if ($forgeProcess.MainWindowHandle -eq 0 -or $forgeProcess.MainWindowTitle -ne 'Forge IDE') { throw 'La fenêtre Forge IDE ne s’est pas initialisée.' }
    Write-Output "Fenêtre native initialisée : $($forgeProcess.MainWindowTitle)"
    if (-not $forgeProcess.CloseMainWindow()) { throw 'La demande de fermeture a échoué.' }
    if (-not $forgeProcess.WaitForExit(10000)) { throw 'La fermeture normale a dépassé 10 secondes.' }
    if ($forgeProcess.ExitCode -ne 0) { throw "Sortie avec code $($forgeProcess.ExitCode)" }
    Write-Output 'Test natif réussi : ouverture et fermeture sans erreur.'
} finally {
    $forgeProcess.Refresh()
    if (-not $forgeProcess.HasExited) { Stop-Process -Id $forgeProcess.Id -Force }
    $forgeProcess.Dispose()
}
