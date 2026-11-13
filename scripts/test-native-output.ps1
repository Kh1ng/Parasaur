param(
  [Parameter(Mandatory = $true)][string]$ReadyFile
)
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$readyPath = (Resolve-Path -LiteralPath $ReadyFile).ProviderPath
$ready = Get-Content -LiteralPath $readyPath -Raw | ConvertFrom-Json
$url = [Uri]$ready.url
if ($url.Scheme -ne 'http' -or $url.Host -ne '127.0.0.1' -or $url.IsDefaultPort -or $url.UserInfo -or $url.AbsolutePath -ne '/native-output-fixture.html' -or $ready.instance -notmatch '^[0-9a-f-]{36}$') {
  throw 'Expected an explicit synthetic loopback fixture address.'
}
$origin = $url.GetLeftPart([UriPartial]::Authority)
$live = Invoke-RestMethod -Uri "$origin/__fixture/config" -TimeoutSec 5
if ($live.instance -ne $ready.instance -or $live.bytes -ne $ready.bytes -or $live.sha256 -ne $ready.sha256 -or $live.bytes -le 16777216) {
  throw 'The live fixture does not match this ready file.'
}
$evidence = Join-Path $repository "artifacts/native-output/$($ready.instance)"
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$log = Join-Path $evidence 'windows.log'
$oldUrl = $env:PARASAUR_WINDOWS_OUTPUT_FIXTURE_URL
try {
  $env:PARASAUR_WINDOWS_OUTPUT_FIXTURE_URL = $ready.url
  & cargo test --manifest-path (Join-Path $repository 'apps/desktop/Cargo.toml') --locked native_output_stream_preserves_files_on_failure_and_cancel -- --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log
  $nativeExit = $LASTEXITCODE
  $resultPath = Join-Path (Split-Path -Parent $readyPath) 'native-output-result.json'
  if (Test-Path -LiteralPath $resultPath) { Copy-Item -LiteralPath $resultPath -Destination (Join-Path $evidence 'native-output-result.json') }
  if ($nativeExit -ne 0) { throw 'Native output acceptance failed. See the retained Windows log.' }
  $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
  if (-not $result.ok -or -not $result.authenticated -or -not $result.serverPreserved -or $result.instance -ne $ready.instance) {
    throw 'Native browser and server evidence did not agree.'
  }
  Write-Output "Native output evidence: $evidence"
} finally {
  $env:PARASAUR_WINDOWS_OUTPUT_FIXTURE_URL = $oldUrl
  try {
    Invoke-RestMethod -Method Post -Uri "$origin/__fixture/stop" -ContentType 'application/json' -Body (@{ instance = $ready.instance } | ConvertTo-Json) -TimeoutSec 5 | Out-Null
  } catch { Write-Warning 'Could not confirm fixture shutdown; inspect its original process handle.' }
}
