[CmdletBinding()]
param(
    [string]$OutputPath = "",
    [string[]]$CursorUserPath = @(),
    [string[]]$CursorExecutablePath = @()
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $baseDirectory = $(if ($PSScriptRoot) { $PSScriptRoot } else { Get-Location })
    $OutputPath = Join-Path $baseDirectory "cursor-cleaner-windows-diagnostics.json"
}

function Redact-Path {
    param([AllowNull()][string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $Value
    }

    $result = $Value
    $replacements = @(
        [pscustomobject]@{ Value = $env:LOCALAPPDATA; Token = "%LOCALAPPDATA%" }
        [pscustomobject]@{ Value = $env:APPDATA; Token = "%APPDATA%" }
        [pscustomobject]@{ Value = $env:USERPROFILE; Token = "%USERPROFILE%" }
        [pscustomobject]@{ Value = $env:TEMP; Token = "%TEMP%" }
    ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_.Value) } |
        Sort-Object { $_.Value.Length } -Descending

    foreach ($replacement in $replacements) {
        $result = [regex]::Replace(
            $result,
            [regex]::Escape($replacement.Value),
            $replacement.Token,
            [Text.RegularExpressions.RegexOptions]::IgnoreCase
        )
    }
    if ($result -eq $Value -and [IO.Path]::IsPathRooted($Value)) {
        $leaf = [IO.Path]::GetFileName($Value.TrimEnd('\', '/'))
        return $(if ($leaf) { "<absolute-path-redacted>\\$leaf" } else { "<absolute-path-redacted>" })
    }
    return $result
}

function Redact-ErrorMessage {
    param([AllowNull()][string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $Value
    }

    $result = Redact-Path $Value
    # Custom absolute paths can occur inside exception messages. Keep the error
    # category useful while removing the rest of any path-like substring.
    return [regex]::Replace(
        $result,
        '(?i)(?:[A-Z]:\\|\\\\)[^\r\n"'']+',
        '<path-redacted>'
    )
}

function Get-CommandVersion {
    param([string]$Name)
    try {
        $command = Get-Command $Name -ErrorAction Stop
        return [string](& $command.Source --version 2>$null | Select-Object -First 1)
    }
    catch {
        return $null
    }
}

function Get-FolderUriShape {
    param([AllowNull()][string]$Folder)
    if ([string]::IsNullOrWhiteSpace($Folder)) { return "empty" }
    if ($Folder -match '^file:///([A-Za-z])%3[Aa]/') { return "file:///<drive>%3A/<redacted>" }
    if ($Folder -match '^file:///([A-Za-z]):/') { return "file:///<drive>:/<redacted>" }
    if ($Folder -match '^file://[^/]') { return "file://<host>/<redacted>" }
    if ($Folder -match '^file:///') { return "file:///<absolute-redacted>" }
    if ($Folder -match '^([A-Za-z]):[\\/]') { return "<drive>:\\<redacted>" }
    if ($Folder -match '^\\\\') { return "\\\\<host>\\<share>\\<redacted>" }
    if ($Folder -match '^([^:]+):') { return "$($Matches[1]):<redacted>" }
    return "relative-or-unknown"
}

function Get-WorkspaceSummary {
    param([string]$Root)
    $storage = Join-Path $Root "workspaceStorage"
    if (-not (Test-Path -LiteralPath $storage -PathType Container)) {
        return [ordered]@{
            path = Redact-Path $storage
            exists = $false
            directory_count = 0
            workspace_json_count = 0
            folder_uri_shapes = @()
        }
    }

    $directories = @(Get-ChildItem -LiteralPath $storage -Directory -Force -ErrorAction SilentlyContinue)
    $workspaceFiles = @($directories | ForEach-Object {
        $candidate = Join-Path $_.FullName "workspace.json"
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { $candidate }
    })
    $shapes = @($workspaceFiles | Select-Object -First 50 | ForEach-Object {
        try {
            $value = Get-Content -LiteralPath $_ -Raw -Encoding UTF8 | ConvertFrom-Json
            Get-FolderUriShape ([string]$value.folder)
        }
        catch {
            "unreadable-json"
        }
    } | Sort-Object -Unique)

    return [ordered]@{
        path = Redact-Path $storage
        exists = $true
        directory_count = $directories.Count
        workspace_json_count = $workspaceFiles.Count
        folder_uri_shapes = $shapes
    }
}

function Get-DatabaseFileSummary {
    param([string]$Path)
    $exists = Test-Path -LiteralPath $Path -PathType Leaf
    $summary = [ordered]@{
        path = Redact-Path $Path
        exists = $exists
        length = $null
        last_write_utc = $null
        sqlite_header = $false
        header_user_version = $null
        header_journal_format = $null
        wal_exists = Test-Path -LiteralPath ($Path + "-wal") -PathType Leaf
        shm_exists = Test-Path -LiteralPath ($Path + "-shm") -PathType Leaf
        schema = $null
        read_error = $null
    }
    if (-not $exists) {
        return $summary
    }

    try {
        $file = Get-Item -LiteralPath $Path
        $summary.length = $file.Length
        $summary.last_write_utc = $file.LastWriteTimeUtc.ToString("o")
        $stream = [IO.File]::Open(
            $Path,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::ReadWrite
        )
        try {
            $header = New-Object byte[] 100
            $read = $stream.Read($header, 0, $header.Length)
            if ($read -ge 64) {
                $summary.sqlite_header = ([Text.Encoding]::ASCII.GetString($header, 0, 16) -eq "SQLite format 3`0")
                if ($summary.sqlite_header) {
                    $summary.header_user_version = [uint32](
                        ([uint32]$header[60] -shl 24) -bor
                        ([uint32]$header[61] -shl 16) -bor
                        ([uint32]$header[62] -shl 8) -bor
                        [uint32]$header[63]
                    )
                    $summary.header_journal_format = $(if ($header[18] -eq 2 -and $header[19] -eq 2) {
                        "wal"
                    } else {
                        "rollback-or-legacy"
                    })
                }
            }
        }
        finally {
            $stream.Dispose()
        }
    }
    catch {
        $summary.read_error = Redact-ErrorMessage $_.Exception.Message
    }
    return $summary
}

function Get-RealDatabasePath {
    param(
        [string]$RedactedPath,
        [string[]]$CandidateRoots
    )
    foreach ($candidateRoot in $CandidateRoots) {
        $redactedRoot = Redact-Path $candidateRoot
        if ($RedactedPath.StartsWith($redactedRoot, [StringComparison]::OrdinalIgnoreCase)) {
            $relative = $RedactedPath.Substring($redactedRoot.Length).TrimStart('\')
            return Join-Path $candidateRoot $relative
        }
    }
    return $null
}

function Convert-ToInt64 {
    param([AllowNull()][string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { return [int64]0 }
    return [int64]$Value
}

function Read-KvDiagnosticsFromCopy {
    param([string]$DatabasePath)

    try {
        $tableRows = [WinSqliteProbe]::Query(
            $DatabasePath,
            @'
SELECT 'ItemTable',
       COUNT(*),
       SUM(CASE WHEN key IS NULL THEN 1 ELSE 0 END),
       SUM(CASE WHEN value IS NULL THEN 1 ELSE 0 END),
       SUM(CASE WHEN typeof(value) = 'text' THEN 1 ELSE 0 END),
       SUM(CASE WHEN typeof(value) = 'blob' THEN 1 ELSE 0 END),
       SUM(CASE WHEN typeof(value) IN ('integer', 'real') THEN 1 ELSE 0 END)
FROM ItemTable
UNION ALL
SELECT 'cursorDiskKV',
       COUNT(*),
       SUM(CASE WHEN key IS NULL THEN 1 ELSE 0 END),
       SUM(CASE WHEN value IS NULL THEN 1 ELSE 0 END),
       SUM(CASE WHEN typeof(value) = 'text' THEN 1 ELSE 0 END),
       SUM(CASE WHEN typeof(value) = 'blob' THEN 1 ELSE 0 END),
       SUM(CASE WHEN typeof(value) IN ('integer', 'real') THEN 1 ELSE 0 END)
FROM cursorDiskKV
ORDER BY 1
'@
        )
        $tables = @($tableRows | ForEach-Object {
            [ordered]@{
                table = [string]$_[0]
                row_count = Convert-ToInt64 $_[1]
                null_key_count = Convert-ToInt64 $_[2]
                null_value_count = Convert-ToInt64 $_[3]
                text_value_count = Convert-ToInt64 $_[4]
                blob_value_count = Convert-ToInt64 $_[5]
                numeric_value_count = Convert-ToInt64 $_[6]
            }
        })

        $cursorCategoryRows = [WinSqliteProbe]::Query(
            $DatabasePath,
            @'
SELECT category, COUNT(*)
FROM (
    SELECT CASE
        WHEN substr(key, 1, length('composerData:')) = 'composerData:'
            THEN 'composerData:<id>'
        WHEN substr(key, 1, length('composerVirtualRowHeights:')) = 'composerVirtualRowHeights:'
            THEN 'composerVirtualRowHeights:<id>'
        WHEN substr(key, 1, length('bubbleId:')) = 'bubbleId:'
            THEN 'bubbleId:<id>:<suffix>'
        WHEN substr(key, 1, length('checkpointId:')) = 'checkpointId:'
            THEN 'checkpointId:<id>:<suffix>'
        WHEN substr(key, 1, length('codeBlockPartialInlineDiffFates:')) = 'codeBlockPartialInlineDiffFates:'
            THEN 'codeBlockPartialInlineDiffFates:<id>:<suffix>'
        WHEN substr(key, 1, length('ofsContent:')) = 'ofsContent:'
            THEN 'ofsContent:<id>:<suffix>'
        ELSE 'other'
    END AS category
    FROM cursorDiskKV
)
GROUP BY category
ORDER BY category
'@
        )
        $cursorCategories = @($cursorCategoryRows | ForEach-Object {
            [ordered]@{ category = [string]$_[0]; count = Convert-ToInt64 $_[1] }
        })

        $itemCategoryRows = [WinSqliteProbe]::Query(
            $DatabasePath,
            @'
SELECT category, COUNT(*)
FROM (
    SELECT CASE
        WHEN substr(key, 1, length('glass/cursor.editorPanelVisibility.agent/')) = 'glass/cursor.editorPanelVisibility.agent/'
            THEN 'glass/cursor.editorPanelVisibility.agent/<id>'
        WHEN substr(key, 1, length('cursor/glass.editorPanelFullscreen/')) = 'cursor/glass.editorPanelFullscreen/'
            THEN 'cursor/glass.editorPanelFullscreen/<id>'
        WHEN substr(key, 1, length('cursor/glass.tabs.v2/')) = 'cursor/glass.tabs.v2/'
             AND key LIKE '%/state.json'
            THEN 'cursor/glass.tabs.v2/<redacted>/<id>/state.json'
        ELSE 'other'
    END AS category
    FROM ItemTable
)
GROUP BY category
ORDER BY category
'@
        )
        $itemCategories = @($itemCategoryRows | ForEach-Object {
            [ordered]@{ category = [string]$_[0]; count = Convert-ToInt64 $_[1] }
        })

        try {
            $composerRows = [WinSqliteProbe]::Query(
                $DatabasePath,
                @'
WITH composer AS (
    SELECT key, value, CAST(value AS TEXT) AS json_text
    FROM cursorDiskKV
    WHERE substr(key, 1, length('composerData:')) = 'composerData:'
)
SELECT COUNT(*),
       SUM(CASE WHEN value IS NULL THEN 1 ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN 1 ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text) = 'object' THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text, '$.composerId') IS NOT NULL THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_extract(json_text, '$.composerId') = substr(key, length('composerData:') + 1) THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text, '$.workspaceIdentifier') IS NOT NULL THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text, '$.title') IS NOT NULL OR json_type(json_text, '$.name') IS NOT NULL OR json_type(json_text, '$.composerTitle') IS NOT NULL THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text, '$.messages') IS NOT NULL THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text, '$.createdAt') IS NOT NULL THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text, '$.lastUpdatedAt') IS NOT NULL OR json_type(json_text, '$.updatedAt') IS NOT NULL THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text, '$.isArchived') IS NOT NULL THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text, '$.isSubagent') IS NOT NULL THEN 1 ELSE 0 END ELSE 0 END),
       SUM(CASE WHEN json_valid(json_text) THEN CASE WHEN json_type(json_text, '$.subComposerIds') IS NOT NULL OR json_type(json_text, '$.subagentComposerIds') IS NOT NULL THEN 1 ELSE 0 END ELSE 0 END)
FROM composer
'@
            )
            $composer = $composerRows[0]
            $composerDiagnostics = [ordered]@{
                available = $true
                row_count = Convert-ToInt64 $composer[0]
                null_value_count = Convert-ToInt64 $composer[1]
                valid_json_count = Convert-ToInt64 $composer[2]
                json_object_count = Convert-ToInt64 $composer[3]
                composer_id_field_count = Convert-ToInt64 $composer[4]
                composer_id_matches_key_count = Convert-ToInt64 $composer[5]
                workspace_identifier_field_count = Convert-ToInt64 $composer[6]
                title_field_count = Convert-ToInt64 $composer[7]
                messages_field_count = Convert-ToInt64 $composer[8]
                created_at_field_count = Convert-ToInt64 $composer[9]
                updated_at_field_count = Convert-ToInt64 $composer[10]
                archived_field_count = Convert-ToInt64 $composer[11]
                subagent_flag_field_count = Convert-ToInt64 $composer[12]
                child_composer_ids_field_count = Convert-ToInt64 $composer[13]
            }
        }
        catch {
            $composerDiagnostics = [ordered]@{
                available = $false
                error = Redact-ErrorMessage $_.Exception.Message
            }
        }

        return [ordered]@{
            available = $true
            privacy = "Aggregate counts only; no database row keys, IDs, titles, message bodies, or JSON values are included."
            tables = $tables
            key_categories = [ordered]@{
                cursorDiskKV = $cursorCategories
                ItemTable = $itemCategories
            }
            composer_data = $composerDiagnostics
        }
    }
    catch {
        return [ordered]@{
            available = $false
            error = Redact-ErrorMessage $_.Exception.Message
        }
    }
}

function Read-SqliteSchemaFromCopy {
    param(
        [string]$DatabasePath,
        [string]$TemporaryRoot
    )
    $copyRoot = Join-Path $TemporaryRoot ([guid]::NewGuid().ToString("N"))
    [void](New-Item -ItemType Directory -Path $copyRoot)
    $destination = Join-Path $copyRoot ([IO.Path]::GetFileName($DatabasePath))
    Copy-Item -LiteralPath $DatabasePath -Destination $destination
    foreach ($suffix in @("-wal", "-shm")) {
        $sidecar = $DatabasePath + $suffix
        if (Test-Path -LiteralPath $sidecar -PathType Leaf) {
            Copy-Item -LiteralPath $sidecar -Destination ($destination + $suffix)
        }
    }

    try {
        $userVersionRows = [WinSqliteProbe]::Query($destination, "PRAGMA user_version")
        $journalRows = [WinSqliteProbe]::Query($destination, "PRAGMA journal_mode")
        $checkRows = [WinSqliteProbe]::Query($destination, "PRAGMA quick_check")
        $tableRows = [WinSqliteProbe]::Query(
            $destination,
            "SELECT name,type FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY type,name"
        )
        $tables = @()
        foreach ($tableRow in $tableRows) {
            $name = [string]$tableRow[0]
            $escaped = $name.Replace('"', '""')
            $columnRows = [WinSqliteProbe]::Query($destination, "PRAGMA table_info(`"$escaped`")")
            $columns = @($columnRows | ForEach-Object {
                [ordered]@{
                    name = [string]$_[1]
                    type = [string]$_[2]
                    not_null = ([string]$_[3] -eq "1")
                    primary_key = ([string]$_[5] -eq "1")
                }
            })
            $tables += [ordered]@{
                name = $name
                kind = [string]$tableRow[1]
                columns = $columns
            }
        }
        $tableNames = @($tableRows | ForEach-Object { [string]$_[0] })
        if (($tableNames -contains "ItemTable") -and ($tableNames -contains "cursorDiskKV")) {
            $kvDiagnostics = Read-KvDiagnosticsFromCopy $destination
        } else {
            $kvDiagnostics = [ordered]@{
                available = $false
                error = "ItemTable or cursorDiskKV is missing"
            }
        }
        return [ordered]@{
            available = $true
            user_version = [int]$userVersionRows[0][0]
            journal_mode = [string]$journalRows[0][0]
            quick_check = [string]$checkRows[0][0]
            tables = $tables
            kv_diagnostics = $kvDiagnostics
        }
    }
    catch {
        return [ordered]@{
            available = $false
            error = Redact-ErrorMessage $_.Exception.Message
        }
    }
    finally {
        Remove-Item -LiteralPath $copyRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$sqliteProviderError = $null
try {
    if (-not ("WinSqliteProbe" -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class WinSqliteProbe
{
    private const int SQLITE_ROW = 100;
    private const int SQLITE_DONE = 101;

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_open16(
        [MarshalAs(UnmanagedType.LPWStr)] string filename,
        out IntPtr database);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_close(IntPtr database);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_prepare16_v2(
        IntPtr database,
        [MarshalAs(UnmanagedType.LPWStr)] string sql,
        int byteCount,
        out IntPtr statement,
        out IntPtr tail);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_step(IntPtr statement);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_finalize(IntPtr statement);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_column_count(IntPtr statement);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr sqlite3_column_text16(IntPtr statement, int column);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr sqlite3_errmsg16(IntPtr database);

    private static string ErrorMessage(IntPtr database)
    {
        IntPtr pointer = sqlite3_errmsg16(database);
        return pointer == IntPtr.Zero ? "unknown SQLite error" : Marshal.PtrToStringUni(pointer);
    }

    public static string[][] Query(string path, string sql)
    {
        IntPtr database = IntPtr.Zero;
        IntPtr statement = IntPtr.Zero;
        IntPtr tail = IntPtr.Zero;
        int result = sqlite3_open16(path, out database);
        if (result != 0)
        {
            string message = database == IntPtr.Zero ? "cannot open SQLite copy" : ErrorMessage(database);
            if (database != IntPtr.Zero) sqlite3_close(database);
            throw new InvalidOperationException(message);
        }

        try
        {
            result = sqlite3_prepare16_v2(database, sql, -1, out statement, out tail);
            if (result != 0) throw new InvalidOperationException(ErrorMessage(database));

            List<string[]> rows = new List<string[]>();
            while ((result = sqlite3_step(statement)) == SQLITE_ROW)
            {
                int count = sqlite3_column_count(statement);
                string[] row = new string[count];
                for (int index = 0; index < count; index++)
                {
                    IntPtr value = sqlite3_column_text16(statement, index);
                    row[index] = value == IntPtr.Zero ? null : Marshal.PtrToStringUni(value);
                }
                rows.Add(row);
            }
            if (result != SQLITE_DONE) throw new InvalidOperationException(ErrorMessage(database));
            return rows.ToArray();
        }
        finally
        {
            if (statement != IntPtr.Zero) sqlite3_finalize(statement);
            sqlite3_close(database);
        }
    }
}
'@
    }
}
catch {
    $sqliteProviderError = Redact-ErrorMessage $_.Exception.Message
}

$cursorProcesses = @(Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -ieq "Cursor" } |
    ForEach-Object { [ordered]@{ name = $_.ProcessName; pid = $_.Id } })

$cursorUserCandidates = @($CursorUserPath)
if ($env:APPDATA) {
    $cursorUserCandidates += Join-Path $env:APPDATA "Cursor\User"
    $cursorUserCandidates += Join-Path $env:APPDATA "Cursor - Insiders\User"
}
$cursorUserCandidates = @($cursorUserCandidates | Select-Object -Unique)

$executableCandidates = @($CursorExecutablePath)
if ($env:LOCALAPPDATA) {
    $executableCandidates += Join-Path $env:LOCALAPPDATA "Programs\cursor\Cursor.exe"
}
if ($env:ProgramFiles) {
    $executableCandidates += Join-Path $env:ProgramFiles "Cursor\Cursor.exe"
}
if (${env:ProgramFiles(x86)}) {
    $executableCandidates += Join-Path ${env:ProgramFiles(x86)} "Cursor\Cursor.exe"
}
$executableCandidates = @($executableCandidates | Select-Object -Unique)
$executables = @($executableCandidates | ForEach-Object {
    $exists = Test-Path -LiteralPath $_ -PathType Leaf
    $version = $null
    if ($exists) { $version = (Get-Item -LiteralPath $_).VersionInfo.ProductVersion }
    [ordered]@{ path = Redact-Path $_; exists = $exists; product_version = $version }
})

$databaseSummaries = @()
$workspaceSummaries = @()
foreach ($userRoot in $cursorUserCandidates) {
    $workspaceSummaries += Get-WorkspaceSummary $userRoot
    $globalStorage = Join-Path $userRoot "globalStorage"
    foreach ($name in @("conversation-search.db", "state.vscdb")) {
        $databaseSummaries += Get-DatabaseFileSummary (Join-Path $globalStorage $name)
    }
}

$projectsRoot = $(if ($env:USERPROFILE) { Join-Path $env:USERPROFILE ".cursor\projects" } else { $null })
$projectsSummary = [ordered]@{
    path = Redact-Path $projectsRoot
    exists = $false
    project_directory_count = 0
    transcript_root_count = 0
    transcript_directory_count = 0
}
if ($projectsRoot -and (Test-Path -LiteralPath $projectsRoot -PathType Container)) {
    $projectDirectories = @(Get-ChildItem -LiteralPath $projectsRoot -Directory -Force -ErrorAction SilentlyContinue)
    $transcriptRoots = @($projectDirectories | ForEach-Object {
        $candidate = Join-Path $_.FullName "agent-transcripts"
        if (Test-Path -LiteralPath $candidate -PathType Container) { $candidate }
    })
    $transcriptCount = 0
    foreach ($root in $transcriptRoots) {
        $transcriptCount += @(Get-ChildItem -LiteralPath $root -Directory -Force -ErrorAction SilentlyContinue).Count
    }
    $projectsSummary.exists = $true
    $projectsSummary.project_directory_count = $projectDirectories.Count
    $projectsSummary.transcript_root_count = $transcriptRoots.Count
    $projectsSummary.transcript_directory_count = $transcriptCount
}

$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) ("cursor-cleaner-diagnostics-" + [guid]::NewGuid().ToString("N"))
[void](New-Item -ItemType Directory -Path $temporaryRoot)
try {
    foreach ($database in $databaseSummaries) {
        if (-not $database.exists -or -not $database.sqlite_header) { continue }
        if ($cursorProcesses.Count -gt 0) {
            $database.schema = [ordered]@{ available = $false; error = "Skipped because Cursor is running" }
            continue
        }
        if ($sqliteProviderError) {
            $database.schema = [ordered]@{ available = $false; error = $sqliteProviderError }
            continue
        }
        $realPath = Get-RealDatabasePath $database.path $cursorUserCandidates
        if ($realPath) {
            $database.schema = Read-SqliteSchemaFromCopy $realPath $temporaryRoot
        }
    }

    $drive = $null
    if ($env:SystemDrive) {
        try {
            $driveInfo = New-Object IO.DriveInfo $env:SystemDrive
            $drive = [ordered]@{
                name = $driveInfo.Name
                drive_type = $driveInfo.DriveType.ToString()
                format = $driveInfo.DriveFormat
            }
        }
        catch {}
    }

    $report = [ordered]@{
        report_version = 3
        generated_utc = [DateTime]::UtcNow.ToString("o")
        privacy = "No conversation bodies, titles, database row keys, IDs, full workspace paths, JSON values, or database files are included. Only aggregate counts and allowlisted field-presence counts are reported."
        operating_system = [ordered]@{
            description = [Runtime.InteropServices.RuntimeInformation]::OSDescription
            architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
            process_architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
            is_64_bit = [Environment]::Is64BitOperatingSystem
            system_drive = $drive
        }
        shell = [ordered]@{
            powershell_version = $PSVersionTable.PSVersion.ToString()
            edition = $PSVersionTable.PSEdition
            host = $Host.Name
            windows_terminal = [bool]$env:WT_SESSION
            term = $env:TERM
            output_encoding = [Console]::OutputEncoding.WebName
        }
        toolchain = [ordered]@{
            rustc = Get-CommandVersion "rustc.exe"
            cargo = Get-CommandVersion "cargo.exe"
            sqlite_provider = "Windows winsqlite3.dll"
            sqlite_provider_error = $sqliteProviderError
        }
        cursor = [ordered]@{
            running_processes = $cursorProcesses
            executables = $executables
            user_directory_candidates = @($cursorUserCandidates | ForEach-Object {
                [ordered]@{ path = Redact-Path $_; exists = Test-Path -LiteralPath $_ -PathType Container }
            })
        }
        databases = $databaseSummaries
        workspaces = $workspaceSummaries
        projects = $projectsSummary
    }

    $fullOutputPath = [IO.Path]::GetFullPath($OutputPath)
    $outputDirectory = Split-Path -Parent $fullOutputPath
    if ($outputDirectory -and -not (Test-Path -LiteralPath $outputDirectory)) {
        [void](New-Item -ItemType Directory -Path $outputDirectory)
    }
    $report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $fullOutputPath -Encoding UTF8
    Write-Host "Diagnostics written to: $fullOutputPath"
    Write-Host "Please review the JSON before sharing it."
}
finally {
    Remove-Item -LiteralPath $temporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
}
