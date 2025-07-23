. ./venv/Scripts/Activate.ps1
# cd ./SerialController/
python -m nuitka --clang --standalone --enable-plugin=tk-inter ./SerialController/Window.py
