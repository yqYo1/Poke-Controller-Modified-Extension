#!/usr/bin/env python3
# -*- coding: utf-8 -*-
from __future__ import annotations
from typing import List, TYPE_CHECKING

from abc import ABCMeta, abstractclassmethod
import tkinter as tk

from PokeConDialogue import PokeConDialogue
from ExternalTools import SocketCommunications, MQTTCommunications

if TYPE_CHECKING:
    from Window import PokeControllerApp
    from Commands.Sender import Sender

# CommandBaseにGUIに関連する関数を集約する。
# print/widget/socket/mqtt関連

class Command:
    __metaclass__ = ABCMeta
    text_area_1 = None
    text_area_2 = None
    stdout_destination = '1'
    isPause = False
    canvas = None
    isGuide = False
    isImage = False
    profilename = None

    def __init__(self):
        self.isRunning = False
        
        self.message_dialogue = None
        self.socket0 = SocketCommunications()
        self.mqtt0 = MQTTCommunications("")

    @abstractclassmethod
    def start(self, ser: Sender, postProcess: PokeControllerApp.stopPlayPost = None):
        pass

    @abstractclassmethod
    def end(self, ser: Sender):
        pass

    def checkIfAlive(self):
        pass

    @abstractclassmethod
    def canvas_test(self):
        top_left = (100, 100)
        bottom_right = (top_left[0] + 200 + 1, top_left[1] + 200 + 1)
        self.canvas.ImgRect(*top_left, *bottom_right, outline="red", tag="neko", ms=2000)

    ############### print functions ###############
    def print_s(self, text: str):
        print(text)

    def print_t1(self, text: int | float | str | list | dict):
        '''
        上側のログ画面に文字列を出力する
        '''
        try:
            self.text_area_1.config(state='normal')
            self.text_area_1.insert('end', str(text) + '\n')
            self.text_area_1.config(state='disable')
            self.text_area_1.see("end")
        except:
            print(text)

    def print_t2(self, text: int | float | str | list | dict):
        '''
        上側のログ画面に文字列を出力する
        '''
        try:
            self.text_area_2.config(state='normal')
            self.text_area_2.insert('end', str(text) + '\n')
            self.text_area_2.config(state='disable')
            self.text_area_2.see("end")
        except:
            print(text)

    def print_t(self, text: int | float | str | list | dict):
        '''
        標準出力先として割り当てられていない方のログ画面に文字列を出力する
        '''
        if self.stdout_destination == '1':
            self.print_t2(text)
        elif self.stdout_destination == '2':
            self.print_t1(text)

    def print_ts(self, text: int | float | str | list | dict):
        '''
        標準出力先として割り当てられている方のログ画面に文字列を出力する
        '''
        if self.stdout_destination == '1':
            self.print_t1(text)
        elif self.stdout_destination == '2':
            self.print_t2(text)

    def print_t1b(self, mode, text: int | float | str | list | dict = ''):
        '''
        上側のログ画面に文字列を出力する
        mode: ['w'/'a'/'d'] 'w'上書き, 'a'追記, 'd'削除
        '''
        try:
            self.text_area_1.config(state='normal')
            if mode in ['w', 'd']:
                self.text_area_1.delete('1.0', 'end')
            if mode == 'w':
                self.text_area_1.insert('1.0', str(text))
            elif mode == 'a':
                self.text_area_1.insert('end', str(text))
            self.text_area_1.config(state='disable')
            self.text_area_1.see("end")
        except:
            pass

    def print_t2b(self, mode, text: int | float | str | list | dict = ''):
        '''
        下側のログ画面に文字列を出力する
        mode: ['w'/'a'/'d'] 'w'上書き, 'a'追記, 'd'削除
        '''
        try:
            self.text_area_2.config(state='normal')
            if mode in ['w', 'd']:
                self.text_area_2.delete('1.0', 'end')
            if mode == 'w':
                self.text_area_2.insert('1.0', str(text))
            elif mode == 'a':
                self.text_area_2.insert('end', str(text))
            self.text_area_2.config(state='disable')
            self.text_area_2.see("end")
        except:
            pass

    def print_tb(self, mode, text: int | float | str | list | dict = ''):
        '''
        標準出力先として割り当てられていない方のログ画面に文字列を出力する
        mode: ['w'/'a'/'d'] 'w'上書き, 'a'追記, 'd'削除
        '''
        if self.stdout_destination == '1':
            self.print_t2b(mode, text)
        elif self.stdout_destination == '2':
            self.print_t1b(mode, text)

    def print_tbs(self, mode, text: int | float | str | list | dict = ''):
        '''
        標準出力先として割り当てられている方のログ画面に文字列を出力する
        mode: ['w'/'a'/'d'] 'w'上書き, 'a'追記, 'd'削除
        '''
        if self.stdout_destination == '1':
            self.print_t1b(mode, text)
        elif self.stdout_destination == '2':
            self.print_t2b(mode, text)

    def dialogue(self, title: str, message: int | str | list, need: type = list) -> list | dict:
        self.message_dialogue = tk.Toplevel()
        ret = PokeConDialogue(self.message_dialogue, title, message).ret_value(need)
        self.message_dialogue = None
        return ret

    def dialogue6widget(self, title: str, dialogue_list: list, need: type = list) -> list | dict:
        self.message_dialogue = tk.Toplevel()
        ret = PokeConDialogue(self.message_dialogue, title, dialogue_list, mode=1).ret_value(need)
        self.message_dialogue = None
        return ret
    
    ############### Socket functions ###############
    def socket_change_alive(self, flug: bool):
        self.socket0.alive = flug

    def socket_change_ipaddr(self, addr: str):
        """
        IPアドレスを変更する
        return:なし
        addr|str:IPアドレス
        """
        self.socket0.change_ipaddr(addr)

    def socket_change_port(self, port: int):
        """
        ポート番号を変更する
        return:なし
        port|int:ポート番号
        """
        self.socket0.change_port(port)
    
    def socket_connect(self):
        """
        socket通信用のserverと接続する
        return:なし
        """
        self.socket0.sock_connect()
    
    def socket_disconnect(self):
        """
        socket通信用のserverから切断する
        return:なし
        """
        self.socket0.sock_disconnect()

    def socket_receive_message(self, header: str, show_msg: bool = False):
        """
        socketを用いて先頭が特定の文字列であるメッセージを受信する
        return output|str:受信した文字列
        header|str:受信したい文字列(先頭)
        show_msg|bool:受信した文字列を出力する
        """
        output = self.socket0.receive_message(header, show_msg=show_msg)
        self.checkIfAlive()
        return output

    def socket_receive_message2(self, headerlist: List[str], show_msg: bool = False):
        """
        socketを用いて先頭が特定の文字列(複数設定可能)であるメッセージを受信する
        return output|str:受信した文字列
        headerlist|list[str]:受信したい文字列(先頭)のリスト
        show_msg|bool:受信した文字列を出力する
        """
        output = self.socket0.receive_message2(headerlist, show_msg=show_msg)
        self.checkIfAlive()
        return output

    def socket_transmit_message(self, message: str):
        """
        socketを用いてメッセージを送信する
        return:なし
        message|str:送信するメッセージ
        """ 
        self.socket0.transmit_message(message)
        self.checkIfAlive()
    
    ############### MQTT functions ###############
    def mqtt_change_broker_address(self, broker_address: str):
        """
        brokerアドレスを変更する
        return:なし
        broker_address|str:brokerアドレス
        """
        self.mqtt0.broker_address = broker_address

    def mqtt_change_id(self, id: str):
        """
        IDを変更する
        return:なし
        id|str:ID
        """
        self.mqtt0.id = id

    def mqtt_change_pub_token(self, pub_token: str):
        """
        pub用tokenを変更する
        return:なし
        pub_token|str:pub用token
        """
        self.mqtt0.pub_token = pub_token

    def mqtt_change_sub_token(self, sub_token: str):
        """
        sub用tokenを変更する
        return:なし
        sub_token|str:sub用token
        """
        self.mqtt0.sub_token = sub_token

    def mqtt_change_clientId(self, clientId: str):
        """
        接続者名を変更する
        return:なし
        clientId|str:接続者名
        """
        self.mqtt0.clientId = clientId

    def mqtt_receive_message(self, roomid: str, header: str, show_msg: bool = False):
        """
        MQTTを用いて先頭が特定の文字列であるメッセージを受信する
        return output|str:受信した文字列
        roomid|str:ROOM ID(topic)
        header|str:受信したい文字列(先頭)
        show_msg|bool:受信した文字列を出力する
        """
        output = self.mqtt0.receive_message(roomid, header, show_msg=show_msg)
        self.checkIfAlive()
        return output

    def mqtt_receive_message2(self, roomid: str, headerlist: str, show_msg: bool = False):
        """
        MQTTを用いて先頭が特定の文字列(複数設定可能)であるメッセージを受信する
        return output|str:受信した文字列
        roomid|str:ROOM ID(topic)
        headerlist|list[str]:受信したい文字列(先頭)のリスト
        show_msg|bool:受信した文字列を出力する
        """
        output = self.mqtt0.receive_message2(roomid, headerlist, show_msg=show_msg)
        self.checkIfAlive()
        return output

    def mqtt_transmit_message(self, roomid: str, message: str):
        """
        MQTTを用いてメッセージを送信する
        return:なし
        roomid|str:ROOM ID(topic)
        message|str:送信するメッセージ
        """   
        self.mqtt0.transmit_message(roomid, message)
        self.checkIfAlive()

if __name__ == "__main__":
    pass