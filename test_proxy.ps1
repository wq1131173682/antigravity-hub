# 测试代理的 Responses API 处理
param(
    [string]$ProxyUrl = "http://127.0.0.1:8045/agnes/v1/responses",
    [string]$Model = "agnes-2.5-flash",
    [string]$Prompt = "Hello, say hi in one word"
)

$body = @{
    model = $Model
    input = $Prompt
    max_output_tokens = 50
} | ConvertTo-Json

Write-Host "=== 发送请求 ===" -ForegroundColor Cyan
Write-Host "URL: $ProxyUrl"
Write-Host "Request: $body" -ForegroundColor Yellow

try {
    $response = Invoke-RestMethod -Uri $ProxyUrl `
        -Method Post `
        -ContentType 'application/json' `
        -Body $body `
        -TimeoutSec 30

    Write-Host "=== 响应 ===" -ForegroundColor Cyan
    $response | ConvertTo-Json -Depth 5
}
catch {
    Write-Host "=== 错误 ===" -ForegroundColor Red
    Write-Host "Status: $($_.Exception.Response.StatusCode.value__)"
    Write-Host "Message: $($_.Exception.Message)"
    try {
        $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
        $reader.BaseStream.Position = 0
        Write-Host "Body: $($reader.ReadToEnd())"
        $reader.Close()
    } catch {
        Write-Host "(无法读取响应体)"
    }
}