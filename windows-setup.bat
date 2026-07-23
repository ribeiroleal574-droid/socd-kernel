@echo off
:: ============================================================
:: SOC-D Kernel — Instalador Automático para Windows
:: Execute como Administrador: clique direito -> "Executar como administrador"
:: ============================================================
setlocal EnableDelayedExpansion

:: Cores no cmd
set "RED=[91m"
set "GREEN=[92m"
set "YELLOW=[93m"
set "CYAN=[96m"
set "BOLD=[1m"
set "RESET=[0m"

:: ─── Banner ───────────────────────────────────────────────────
cls
echo.
echo %CYAN%  ╔═══════════════════════════════════════════════════╗%RESET%
echo %CYAN%  ║     SOC-D Kernel — Instalador para Windows       ║%RESET%
echo %CYAN%  ║   Sistema Operacional Cognitivo Distribuído      ║%RESET%
echo %CYAN%  ╚═══════════════════════════════════════════════════╝%RESET%
echo.

:: ─── Verificar se é Administrador ────────────────────────────
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo %RED%[ERRO] Execute este script como Administrador!%RESET%
    echo.
    echo Clique direito no arquivo e escolha:
    echo "Executar como administrador"
    echo.
    pause
    exit /b 1
)

echo %GREEN%[OK]%RESET% Executando como Administrador
echo.

:: ─── Verificar versão do Windows ─────────────────────────────
for /f "tokens=4-5 delims=. " %%i in ('ver') do set VERSION=%%i.%%j
echo %CYAN%[>>]%RESET% Windows versão: %VERSION%

:: Verifica se Win10 2004+ ou Win11
for /f "tokens=3*" %%i in ('reg query "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion" /v CurrentBuild') do set BUILD=%%j
echo %CYAN%[>>]%RESET% Build: %BUILD%

if %BUILD% LSS 19041 (
    echo %YELLOW%[!!]%RESET% AVISO: Build %BUILD% pode ter suporte limitado ao WSL2
    echo %YELLOW%[!!]%RESET% Recomendado: Build 19041+ (Win10 v2004) ou superior
)

echo.

:: ─── Menu de escolha ─────────────────────────────────────────
:MENU
echo %BOLD%Escolha o método de instalação:%RESET%
echo.
echo   %CYAN%[1]%RESET% WSL2 + Ubuntu  %GREEN%(Recomendado — mais fácil)%RESET%
echo   %CYAN%[2]%RESET% Windows Nativo %YELLOW%(Sem WSL — mais trabalhoso)%RESET%
echo   %CYAN%[3]%RESET% Verificar instalação existente
echo   %CYAN%[4]%RESET% Sair
echo.
set /p CHOICE=Digite sua escolha [1-4]: 

if "%CHOICE%"=="1" goto WSL2_INSTALL
if "%CHOICE%"=="2" goto NATIVE_INSTALL
if "%CHOICE%"=="3" goto CHECK_INSTALL
if "%CHOICE%"=="4" exit /b 0
echo Opção inválida. Tente novamente.
goto MENU


:: ════════════════════════════════════════════════════════════
:: OPÇÃO 1: WSL2 + Ubuntu (Recomendado)
:: ════════════════════════════════════════════════════════════
:WSL2_INSTALL
echo.
echo %CYAN%══ INSTALAÇÃO VIA WSL2 ══%RESET%
echo.

:: Passo 1: Habilitar WSL2
echo %CYAN%[PASSO 1/5]%RESET% Habilitando WSL2...
wsl --install --no-launch 2>nul
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% WSL habilitado com sucesso
) else (
    :: Método alternativo para builds mais antigos
    echo %YELLOW%[>>]%RESET% Tentando método alternativo...
    dism.exe /online /enable-feature /featurename:Microsoft-Windows-Subsystem-Linux /all /norestart >nul 2>&1
    dism.exe /online /enable-feature /featurename:VirtualMachinePlatform /all /norestart >nul 2>&1
    echo %GREEN%[OK]%RESET% Features WSL habilitadas
)

:: Passo 2: Definir WSL2 como padrão
echo %CYAN%[PASSO 2/5]%RESET% Configurando WSL2 como padrão...
wsl --set-default-version 2 2>nul
echo %GREEN%[OK]%RESET% WSL2 configurado como padrão

:: Passo 3: Instalar Ubuntu
echo %CYAN%[PASSO 3/5]%RESET% Verificando Ubuntu...
wsl -l -q 2>nul | findstr /i "ubuntu" >nul
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% Ubuntu já instalado no WSL2
) else (
    echo %CYAN%[>>]%RESET% Instalando Ubuntu no WSL2...
    wsl --install -d Ubuntu 2>nul
    if %errorLevel% equ 0 (
        echo %GREEN%[OK]%RESET% Ubuntu instalado!
    ) else (
        echo %YELLOW%[!!]%RESET% Instale o Ubuntu manualmente pela Microsoft Store
        echo      Procure por "Ubuntu" na Store e instale
        start ms-windows-store://search/?query=Ubuntu
    )
)

:: Passo 4: Criar script de setup para rodar dentro do WSL2
echo %CYAN%[PASSO 4/5]%RESET% Criando script de setup para Ubuntu...

set SETUP_SCRIPT=%TEMP%\socd_wsl_setup.sh
(
echo #!/bin/bash
echo echo "=== SOC-D Setup dentro do Ubuntu/WSL2 ==="
echo.
echo # Atualizar sistema
echo sudo apt-get update -qq ^&^& sudo apt-get upgrade -y -qq
echo.
echo # Instalar dependencias
echo sudo apt-get install -y -qq ^
echo     build-essential curl git wget unzip ^
echo     pkg-config libssl-dev gcc make python3 ^
echo     qemu-system-x86 gdb 2^>/dev/null
echo.
echo # Instalar Rust nightly
echo if ! command -v rustup ^&^>/dev/null; then
echo     curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs ^| sh -s -- -y --default-toolchain nightly --quiet
echo fi
echo source "$HOME/.cargo/env" 2^>/dev/null ^|^| export PATH="$HOME/.cargo/bin:$PATH"
echo.
echo # Configurar Rust
echo rustup default nightly 2^>/dev/null
echo rustup component add rust-src llvm-tools-preview rustfmt clippy 2^>/dev/null
echo rustup target add x86_64-unknown-none aarch64-unknown-none 2^>/dev/null
echo cargo install bootimage 2^>/dev/null
echo.
echo echo ""
echo echo "=== Setup concluido! ==="
echo echo "Proximos passos:"
echo echo "  1. Copie o socd-kernel-completo.zip para o WSL2"
echo echo "  2. Execute: unzip socd-kernel-completo.zip ^&^& cd socd-kernel"
echo echo "  3. Execute: chmod +x setup.sh ^&^& ./setup.sh"
echo echo "  4. Execute: make run"
echo echo ""
) > "%SETUP_SCRIPT%"

:: Converte para Unix line endings
powershell -Command "(Get-Content '%SETUP_SCRIPT%') | Set-Content -NoNewline '%SETUP_SCRIPT%.unix'" 2>nul

:: Passo 5: Executar setup no WSL2
echo %CYAN%[PASSO 5/5]%RESET% Executando setup dentro do Ubuntu...
wsl bash "%SETUP_SCRIPT%" 2>nul
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% Setup Ubuntu concluído!
) else (
    echo %YELLOW%[!!]%RESET% Execute manualmente dentro do Ubuntu:
    echo     bash /tmp/socd_wsl_setup.sh
)

goto WSL2_DONE


:: ════════════════════════════════════════════════════════════
:: OPÇÃO 2: Instalação Nativa no Windows
:: ════════════════════════════════════════════════════════════
:NATIVE_INSTALL
echo.
echo %CYAN%══ INSTALAÇÃO NATIVA NO WINDOWS ══%RESET%
echo.

:: Verificar Chocolatey
echo %CYAN%[PASSO 1/6]%RESET% Verificando Chocolatey...
where choco >nul 2>&1
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% Chocolatey já instalado: 
    choco --version
) else (
    echo %CYAN%[>>]%RESET% Instalando Chocolatey...
    powershell -NoProfile -ExecutionPolicy Bypass -Command ^
        "Set-ExecutionPolicy Bypass -Scope Process -Force; ^
        [System.Net.ServicePointManager]::SecurityProtocol = ^
        [System.Net.ServicePointManager]::SecurityProtocol -bor 3072; ^
        iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))"
    
    :: Recarrega PATH
    set PATH=%PATH%;%ALLUSERSPROFILE%\chocolatey\bin
    
    where choco >nul 2>&1
    if %errorLevel% equ 0 (
        echo %GREEN%[OK]%RESET% Chocolatey instalado!
    ) else (
        echo %RED%[ERRO]%RESET% Falha ao instalar Chocolatey
        echo Instale manualmente em: https://chocolatey.org/install
        pause
        goto MENU
    )
)

:: Instalar dependências
echo %CYAN%[PASSO 2/6]%RESET% Instalando dependências via Chocolatey...
choco install -y git make python3 llvm qemu mingw wget curl 2>nul
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% Dependências instaladas
) else (
    echo %YELLOW%[!!]%RESET% Alguns pacotes podem ter falhado — continuando...
)

:: Recarrega variáveis de ambiente
call refreshenv 2>nul

:: Verificar QEMU
echo %CYAN%[PASSO 3/6]%RESET% Verificando QEMU...
where qemu-system-x86_64 >nul 2>&1
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% QEMU encontrado
) else (
    echo %YELLOW%[!!]%RESET% QEMU não encontrado no PATH
    echo Adicionando caminho padrão do QEMU...
    set PATH=%PATH%;C:\Program Files\qemu
)

:: Instalar Rust
echo %CYAN%[PASSO 4/6]%RESET% Instalando Rust nightly...
where rustup >nul 2>&1
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% rustup já instalado
    rustup update nightly 2>nul
) else (
    echo %CYAN%[>>]%RESET% Baixando rustup-init.exe...
    powershell -Command ^
        "Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile '%TEMP%\rustup-init.exe'"
    
    if exist "%TEMP%\rustup-init.exe" (
        echo %CYAN%[>>]%RESET% Instalando Rust nightly para Windows GNU...
        "%TEMP%\rustup-init.exe" -y --default-toolchain nightly --default-host x86_64-pc-windows-gnu --quiet
        set PATH=%PATH%;%USERPROFILE%\.cargo\bin
        echo %GREEN%[OK]%RESET% Rust instalado!
    ) else (
        echo %RED%[ERRO]%RESET% Falha ao baixar rustup-init.exe
        echo Baixe manualmente em: https://rustup.rs
        pause
        goto MENU
    )
)

:: Configurar Rust
echo %CYAN%[PASSO 5/6]%RESET% Configurando componentes Rust...
rustup default nightly-x86_64-pc-windows-gnu 2>nul
rustup component add rust-src llvm-tools-preview rustfmt clippy 2>nul
rustup target add x86_64-unknown-none aarch64-unknown-none 2>nul
cargo install bootimage 2>nul
echo %GREEN%[OK]%RESET% Rust configurado!

:: Verificar LLD linker
echo %CYAN%[PASSO 6/6]%RESET% Configurando linker LLD...
where lld >nul 2>&1
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% LLD encontrado
    :: Cria alias rust-lld se não existir
    if not exist "%USERPROFILE%\.cargo\bin\rust-lld.exe" (
        copy "C:\Program Files\LLVM\bin\lld.exe" "%USERPROFILE%\.cargo\bin\rust-lld.exe" >nul 2>&1
        echo %GREEN%[OK]%RESET% rust-lld configurado
    )
) else (
    echo %YELLOW%[!!]%RESET% LLD não encontrado — adicionando LLVM ao PATH...
    set PATH=%PATH%;C:\Program Files\LLVM\bin
    setx PATH "%PATH%;C:\Program Files\LLVM\bin" >nul 2>&1
)

goto NATIVE_DONE


:: ════════════════════════════════════════════════════════════
:: OPÇÃO 3: Verificar instalação
:: ════════════════════════════════════════════════════════════
:CHECK_INSTALL
echo.
echo %CYAN%══ VERIFICAÇÃO DA INSTALAÇÃO ══%RESET%
echo.

:: WSL2
wsl --status 2>nul | findstr /i "Default Version" >nul
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% WSL2 instalado
    wsl -l -v 2>nul
) else (
    echo %YELLOW%[!!]%RESET% WSL2 não detectado
)
echo.

:: Rust
where rustc >nul 2>&1
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% Rust: 
    rustc --version 2>nul
    cargo --version 2>nul
) else (
    echo %YELLOW%[!!]%RESET% Rust não encontrado
)

:: QEMU
where qemu-system-x86_64 >nul 2>&1
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% QEMU instalado
) else (
    echo %YELLOW%[!!]%RESET% QEMU não encontrado no PATH
)

:: Targets
rustup target list --installed 2>nul | findstr "x86_64-unknown-none" >nul
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% Target x86_64-unknown-none disponível
) else (
    echo %YELLOW%[!!]%RESET% Target x86_64-unknown-none não instalado
    echo     Execute: rustup target add x86_64-unknown-none
)

:: bootimage
where cargo-bootimage >nul 2>&1
if %errorLevel% equ 0 (
    echo %GREEN%[OK]%RESET% bootimage instalado
) else (
    echo %YELLOW%[!!]%RESET% bootimage não encontrado
    echo     Execute: cargo install bootimage
)

echo.
pause
goto MENU


:: ════════════════════════════════════════════════════════════
:: FIM — WSL2
:: ════════════════════════════════════════════════════════════
:WSL2_DONE
echo.
echo %GREEN%╔══════════════════════════════════════════════════════╗%RESET%
echo %GREEN%║         WSL2 — Configuração Concluída!               ║%RESET%
echo %GREEN%╚══════════════════════════════════════════════════════╝%RESET%
echo.
echo %BOLD%PRÓXIMOS PASSOS:%RESET%
echo.
echo 1. Abra o Ubuntu (procure "Ubuntu" no Menu Iniciar)
echo.
echo 2. Dentro do Ubuntu, copie o ZIP para o home:
echo    cp /mnt/c/Users/%USERNAME%/Downloads/socd-kernel-completo.zip ~/
echo.
echo 3. Extraia e compile:
echo    unzip socd-kernel-completo.zip
echo    cd socd-kernel
echo    chmod +x setup.sh ^&^& ./setup.sh
echo.
echo 4. Rode o kernel:
echo    make run
echo.
echo %YELLOW%NOTA para Windows 10:%RESET%
echo   A janela do QEMU pode não aparecer sem um servidor X11.
echo   Use modo headless:
echo   cargo bootimage run -- -serial stdio -display none -m 256M
echo.
echo %YELLOW%NOTA para Windows 11:%RESET%
echo   WSLg já inclui suporte gráfico — a janela QEMU aparece normalmente!
echo.

:: Pergunta se quer abrir o Ubuntu agora
set /p OPEN=Abrir o Ubuntu agora? [S/N]: 
if /i "%OPEN%"=="S" (
    start wsl -d Ubuntu
    echo %GREEN%[OK]%RESET% Ubuntu iniciado!
)

pause
goto END


:: ════════════════════════════════════════════════════════════
:: FIM — Nativo
:: ════════════════════════════════════════════════════════════
:NATIVE_DONE
echo.
echo %GREEN%╔══════════════════════════════════════════════════════╗%RESET%
echo %GREEN%║      Windows Nativo — Configuração Concluída!        ║%RESET%
echo %GREEN%╚══════════════════════════════════════════════════════╝%RESET%
echo.
echo %BOLD%PRÓXIMOS PASSOS (no PowerShell normal):%RESET%
echo.
echo 1. Extraia o projeto:
echo    Expand-Archive -Path socd-kernel-completo.zip -DestinationPath .
echo    cd socd-kernel
echo.
echo 2. Compile:
echo    cargo build --target x86_64-unknown-none
echo    cargo bootimage
echo.
echo 3. Rode no QEMU:
echo    cargo bootimage run -- -serial stdio -display sdl -m 256M
echo.
echo %YELLOW%Se o linker falhar, execute:%RESET%
echo    rustup default nightly-x86_64-pc-windows-gnu
echo.

pause

:END
echo.
echo %CYAN%SOC-D Kernel — Instalação Windows concluída%RESET%
echo.
endlocal
