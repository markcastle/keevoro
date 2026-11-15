# Installs or updates Keevoro on Windows from the latest GitHub Release.
#
# While the repository is private, set $env:GITHUB_TOKEN to a token that can
# read it, then in PowerShell:
#
#   irm https://api.github.com/repos/markcastle/keevoro/contents/install.ps1 -Headers @{ Authorization = "Bearer $env:GITHUB_TOKEN"; Accept = 'application/vnd.github.raw' } | iex
#
# Once it is public this is enough:
#
#   irm https://raw.githubusercontent.com/markcastle/keevoro/main/install.ps1 | iex
#
# It downloads keevoro.exe, checks it against the release's checksum file,
# puts it in your own Programs folder and adds that folder to your PATH. No
# administrator prompt. Running it again updates to the newest version; so
# does `keevoro update`.

$ErrorActionPreference = 'Stop'
$repo = 'markcastle/keevoro'
$assetName = 'keevoro-windows-x64.exe'
$installDir = if ($env:KEEVORO_INSTALL_DIR) { $env:KEEVORO_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\Keevoro' }

$headers = @{ 'User-Agent' = 'keevoro-install' }
if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }

Write-Host 'Looking up the latest Keevoro release...'
try {
    $release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers $headers
} catch {
    throw "Could not read the latest release of $repo. If the repository is private, set GITHUB_TOKEN first. ($($_.Exception.Message))"
}

$asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
$sums = $release.assets | Where-Object { $_.name -eq 'SHA256SUMS' } | Select-Object -First 1
if (-not $asset -or -not $sums) { throw "Release $($release.tag_name) has no Windows build." }

# Files are fetched through the API address, which works whether or not the
# repository is private.
$download = $headers.Clone(); $download['Accept'] = 'application/octet-stream'
$tmp = Join-Path $env:TEMP "keevoro-$($release.tag_name)-$PID.exe"
$tmpSums = "$tmp.sums"
Write-Host "Downloading Keevoro $($release.tag_name) ($([math]::Round($asset.size / 1MB, 1)) MB)..."
Invoke-WebRequest $asset.url -OutFile $tmp -Headers $download -UseBasicParsing
Invoke-WebRequest $sums.url -OutFile $tmpSums -Headers $download -UseBasicParsing

# Never install a download that does not match the release's checksum.
$line = Get-Content $tmpSums | Where-Object { $_ -match "\s\*?$([regex]::Escape($assetName))$" } | Select-Object -First 1
$expected = if ($line) { ($line -split '\s+')[0].ToLower() } else { '' }
$actual = (Get-FileHash $tmp -Algorithm SHA256).Hash.ToLower()
Remove-Item $tmpSums -ErrorAction SilentlyContinue
if (-not $expected -or $expected -ne $actual) {
    Remove-Item $tmp -ErrorAction SilentlyContinue
    throw "The download did not match the release's checksum, so nothing was installed. Try again."
}
# The file came from the internet; without this Windows asks before running it.
Unblock-File $tmp

New-Item -ItemType Directory -Force -Path $installDir | Out-Null
$exe = Join-Path $installDir 'keevoro.exe'
try {
    Move-Item -Force $tmp $exe
} catch {
    Remove-Item $tmp -ErrorAction SilentlyContinue
    throw "Could not replace $exe. If Keevoro is running, let it finish and run this again. ($($_.Exception.Message))"
}

# Put the folder on this user's PATH, once, and on this window's PATH too so
# it works straight away.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not $userPath) { $userPath = '' }
if (($userPath -split ';') -notcontains $installDir) {
    [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ';' + $installDir).TrimStart(';'), 'User')
    Write-Host "Added $installDir to your PATH. New terminal windows will find keevoro."
}
if (($env:Path -split ';') -notcontains $installDir) { $env:Path = "$env:Path;$installDir" }

Write-Host "Keevoro $($release.tag_name) is installed. Try it:  keevoro --help"
