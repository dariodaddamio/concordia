[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('codex', 'claude', 'cursor')]
    [string] $Provider,
    [Parameter(Mandatory = $true)]
    [string] $CliPath,
    [string] $Version = 'synthetic-review'
)

Write-Error "Fixture refresh is manual and intentionally does not launch provider CLIs. Capture output in an isolated test home, redact it, and review it before placing JSONL under tests/fixtures/streams/$Provider/$Version/."
exit 2
