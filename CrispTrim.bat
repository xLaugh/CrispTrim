@echo off
rem Lance CrispTrim dans sa fenêtre (double-clic).
cd /d "%~dp0"
uv run --extra asr crisptrim-app
if errorlevel 1 pause
