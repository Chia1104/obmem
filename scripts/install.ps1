# Installs the obmem binary of a GitHub release on Windows.
#
#   irm https://github.com/Chia1104/obmem/releases/latest/download/install.ps1 | iex
#
# It downloads the archive, checks it against the release's SHA256SUMS and puts obmem.exe in
# %USERPROFILE%\.obmem\bin. Nothing else on the machine is changed: when that folder is not on
# PATH, it says how to add it.
#
#   $env:OBMEM_VERSION       the release to install, such as 0.1.0 (default: the latest)
#   $env:OBMEM_INSTALL_DIR   where the binary goes (default: %USERPROFILE%\.obmem\bin)
#   $env:OBMEM_RELEASE_URL   the folder that holds a release's files, for a mirror or a test
#                            (default: that release on GitHub)

$ErrorActionPreference = 'Stop'

# In a function: a download that stops halfway defines nothing and runs nothing.
function Install-Obmem {
    $repository = 'Chia1104/obmem'
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw 'obmem install: there is no prebuilt binary for 32-bit Windows'
    }
    # The x64 binary also runs on Windows on Arm, which emulates it.
    $archive = 'obmem-x86_64-pc-windows-msvc.zip'

    $version = if ($env:OBMEM_VERSION) { $env:OBMEM_VERSION -replace '^v', '' } else { 'latest' }
    $url = if ($env:OBMEM_RELEASE_URL) {
        $env:OBMEM_RELEASE_URL
    } elseif ($version -eq 'latest') {
        "https://github.com/$repository/releases/latest/download"
    } else {
        "https://github.com/$repository/releases/download/v$version"
    }
    $dir = if ($env:OBMEM_INSTALL_DIR) { $env:OBMEM_INSTALL_DIR } else { Join-Path $env:USERPROFILE '.obmem\bin' }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("obmem-install-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        # Without the progress bar, Windows PowerShell 5 downloads many times faster.
        $ProgressPreference = 'SilentlyContinue'
        Invoke-WebRequest -UseBasicParsing "$url/$archive" -OutFile (Join-Path $tmp $archive)
        Invoke-WebRequest -UseBasicParsing "$url/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')

        $expected = Get-Content (Join-Path $tmp 'SHA256SUMS') |
            ForEach-Object { , ($_ -split '\s+', 2) } |
            Where-Object { $_.Count -eq 2 -and $_[1].TrimStart('*') -eq $archive } |
            ForEach-Object { $_[0] } |
            Select-Object -First 1
        if (-not $expected) {
            throw "obmem install: SHA256SUMS of this release does not list $archive"
        }
        $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $archive)).Hash
        if ($actual -ne $expected) {
            throw "obmem install: $archive does not match its checksum (expected $expected, got $actual)"
        }

        Expand-Archive (Join-Path $tmp $archive) -DestinationPath $tmp
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $exe = Join-Path $dir 'obmem.exe'
        # Windows does not replace a program that is running, but it lets it be renamed.
        if (Test-Path $exe) {
            $old = "$exe.old"
            Remove-Item $old -Force -ErrorAction SilentlyContinue
            Move-Item $exe $old -Force
        }
        Move-Item (Join-Path $tmp 'obmem.exe') $exe
        Remove-Item "$exe.old" -Force -ErrorAction SilentlyContinue
    } finally {
        Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Output "installed $(& $exe --version) to $exe"
    $found = Get-Command obmem -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $found) {
        Write-Output "$dir is not on PATH. Add it for your user with:"
        Write-Output "  [Environment]::SetEnvironmentVariable('Path', `"$dir;`" + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')"
    } elseif ($found.Source -ne $exe) {
        Write-Output "note: ``obmem`` on PATH is $($found.Source), another install that comes before $dir"
    }
}

Install-Obmem
