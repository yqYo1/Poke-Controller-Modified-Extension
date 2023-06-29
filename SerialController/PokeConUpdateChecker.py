#!/usr/bin/env python3
# -*- coding: utf-8 -*-
from __future__ import annotations

import argparse
import os
import shutil
from git import Repo, GitCommandError
import datetime
import tkinter as tk
from tkinter import messagebox
import webbrowser

class PokeConUpdateCheck(object):
    def __init__(self, mode="0"):
        self.pokecon_path = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        print(self.pokecon_path)

    def check_repository_updates(self):
        try:
            repo = Repo(self.pokecon_path)

            # リモートリポジトリの更新を取得
            origin = repo.remotes.origin
            origin.fetch()

            # ローカルブランチの最新コミット
            local_branch = repo.active_branch
            local_latest_commit = local_branch.commit

            # リモートブランチの最新コミット
            remote_branch = origin.refs[local_branch.name]
            remote_latest_commit = remote_branch.commit

            # ローカルとリモートの最新コミットを比較
            if local_latest_commit == remote_latest_commit:
                print("リポジトリは最新です。")
                return "0"
            else:
                print("リポジトリに更新があります。")
                return "1"
        except:
            print("リポジトリにアクセスできませんでした。")
            return "-1"
    
    def get_conflicting_files(self):
        repo = Repo(self.pokecon_path)
        conflicting_files = []

        try:
            repo.git.pull()
            print("更新完了")
        except GitCommandError as e:
            error_output = e.stderr
            print(e)
            if "Your local changes to the following files would be overwritten by merge" in error_output:
                start_index = error_output.index("Your local changes to the following files would be overwritten by merge")
                end_index = error_output.index("Please commit your changes or stash them before you merge.")
                conflicting_files_text = error_output[start_index:end_index].splitlines()[1:]
                conflicting_files = [file.strip() for file in conflicting_files_text]

        return conflicting_files
    
    def git_pull(self):
        repo = Repo(self.pokecon_path)
        try:
            repo.git.pull()
            print("更新完了")
        except:
            pass

if __name__ == "__main__":

    parser = argparse.ArgumentParser(description='Switch/GC automation support software using Python')
    parser.add_argument('--msgbox', '-m', help='show_msgbox', action="store_true")
    args = parser.parse_args()

    root = tk.Tk()
    root.withdraw()  # メインウィンドウを非表示にする
    if args.msgbox:
        res = messagebox.askyesno(title="更新確認", message="Poke-Controller Modified Extension の更新を確認しますか?")
    else:
        res = True
    if res:
        res_check = PokeConUpdateCheck().check_repository_updates()
        if res_check == "0":
            if args.msgbox:
                res = tk.messagebox.showinfo(title="更新確認", message="更新はありませんでした。")
        elif res_check == "1":
            txt = '【注意1】\n'\
            '以下のディレクトリ内のファイルは更新されません。\n'\
            '・PythonCommands (Samplesディレクトリを除く)\n'\
            '・Template (Samplesディレクトリを除く)\n'\
            '・Captures\n'\
            '・Controller_Log\n'\
            '・profiles\n'\
            '【注意2】\n'\
            '更新対象のファイルのうちユーザーが手動で更新したファイルはoldディレクトリに移動されます。'
            res_update = messagebox.askyesno(title="更新確認", message="最新版が公開されています。更新しますか?", detail=txt)
            if res_update:
                try:
                    conflicting_files = PokeConUpdateCheck().get_conflicting_files()
                    if len(conflicting_files) > 0:
                        pathname = os.path.join("old", datetime.datetime.today().strftime("%Y%m%d%H%M%S"))
                        os.makedirs(pathname)
                        for i in conflicting_files:
                            filename = os.path.join(pathname, os.path.basename(i))
                            shutil.copy2(i, filename)
                            if os.path.exists(filename):
                                os.remove(i)    
                        PokeConUpdateCheck().git_pull()
                    tk.messagebox.showinfo(title="更新確認", message="更新が完了しました。")
                except:
                    tk.messagebox.showerror(title="更新確認", message="更新に失敗しました。\手動でgithubから必要なファイルをダウンロードしてください。")
                    webbrowser.open("https://github.com/futo030/Poke-Controller-Modified-Extension", 2)
        else:
            if args.msgbox:
                res = tk.messagebox.showwarning(title="更新確認", message="確認できませんでした。")
