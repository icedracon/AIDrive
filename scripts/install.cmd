@echo off
rem Installs or updates AIDrive for this Windows user from this folder.
rem Run it in your project folder: "C:\path\to\AIDrive\install.cmd"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
