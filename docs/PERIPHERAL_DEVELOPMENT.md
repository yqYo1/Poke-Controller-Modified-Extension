# PokeCon周辺機器開発ガイド

この文書は、PokeConのシリアル通信先となるMCU firmware、USB serial bridge、protocol analyzerを作る開発者を対象にします。

一般利用者がportを選ぶ手順は[利用ガイド](USER_GUIDE.md)で説明します。

ユーザースクリプトの`McuCommand`とraw serial APIは[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md)で説明します。

## software contractと電気的設計を分離する

PokeConが規定するのはOSのserial portへ書くbyte列とconnection lifecycleです。

MCUの動作電圧、pin配置、level変換、USB descriptor、対象consoleへの配線は規定しません。

周辺機器開発者は使用するboard、console、adapterの公式資料に従って電気的安全性を設計します。

対象consoleや周辺機器の保護回路を省略しません。

実機へ接続する前に、logic analyzerまたはPTYでPokeCon側のbyte列を確認します。

## port、baud rate、data formatを一致させる

PokeConが公開設定として持つserial項目は次の3個です。

| 設定ID | 役割 | default |
|---|---|---|
| `serial.port` | Linux device pathまたはWindows COM名 | 空文字 |
| `serial.baud_rate` | 1以上のbaud rate | `9600` |
| `serial.data_format` | controller frame形式 | `default` |

`serial.data_format`は`default`、`qingpi`、`3ds`から選択します。

UIで`3ds`を選ぶとbaud rateも便宜上115200へ変更します。

CLI、TOML、環境変数、動的設定ではformatを変更してもbaud rateを自動変更しません。

firmwareとPokeConの両方でbaud rateを明示的に確認します。

data bits、parity、stop bits、flow controlは現在の公開設定に含まれません。

PokeConのnative adapterは固定した`tokio-serial` versionのbuilder defaultを使用しますが、そのdefaultを独自protocolの恒久的な拡張点として扱いません。

対象releaseとOS driverを使った接続試験でline disciplineを確認します。

## canonical controller stateを理解する

3種類のencoderへ渡す共通stateは次の要素を持ちます。

| 要素 | canonical範囲 | neutral |
|---|---|---|
| button | 14個のboolean | 全てfalse |
| hat | 8方向とneutral | `neutral` |
| left stick | `x`と`y`が0から255 | `(128, 128)` |
| right stick | `x`と`y`が0から255 | `(128, 128)` |
| touch | `x`が0から319、`y`が0から239、または未押下 | 未押下 |

UIと動的入力のstickは103から153のdead zoneを128へ正規化する経路があります。

firmwareはwire上で受信した0から255の値を再度dead zone処理するかを独自に決めます。

PokeCon wire encoder自体はcodecへ渡されたcanonical byte値を使用します。

## Switch button maskを実装する

`default`と`qingpi`は同じ14-bitの**Switch button mask**を使用します。

「Switch button mask」のbit割当は次のとおりです。

| bit | button | mask |
|---:|---|---:|
| 0 | Y | `0x0001` |
| 1 | B | `0x0002` |
| 2 | A | `0x0004` |
| 3 | X | `0x0008` |
| 4 | L | `0x0010` |
| 5 | R | `0x0020` |
| 6 | ZL | `0x0040` |
| 7 | ZR | `0x0080` |
| 8 | MINUS | `0x0100` |
| 9 | PLUS | `0x0200` |
| 10 | LCLICK | `0x0400` |
| 11 | RCLICK | `0x0800` |
| 12 | HOME | `0x1000` |
| 13 | CAPTURE | `0x2000` |

複数buttonはbitwise ORで結合します。

未定義のbit 14と15は0です。

## Switch hat値を実装する

`default`と`qingpi`はhatを次の1-byte値で表します。

| 値 | direction |
|---:|---|
| 0 | Up |
| 1 | UpRight |
| 2 | Right |
| 3 | DownRight |
| 4 | Down |
| 5 | DownLeft |
| 6 | Left |
| 7 | UpLeft |
| 8 | Neutral |

9以上はPokeConが生成する正規frameには現れません。

## default text frameをparseする

`default`はASCII textをCRLFで終端する可変長frameです。

基本形は次のとおりです。

```text
0xFFFFFF H [LX LY] [RX RY]\r\n
```

先頭は小文字`0x`と、ちょうど6桁の小文字16進数です。

その後にspaceとhatの10進値を置きます。

stick byteは必要な場合だけ、各byteを2桁の小文字16進数としてspace区切りで追加します。

flagsのbit layoutは次のとおりです。

| flags bit | 意味 |
|---:|---|
| 0 | right stick pairを含む |
| 1 | left stick pairを含む |
| 2から15 | Switch button maskを2-bit left shiftした値 |

button maskは`(flags >> 2) & 0x3fff`で復元します。

bit 1が立っていればleft stick pairが先に続きます。

bit 0が立っていればright stick pairがその後に続きます。

両方が0ならstick byteはありません。

firmwareは最後に受理したstick stateを保持し、省略されたstickを変更しません。

## default frameのdeltaを正しくcommitする

PokeConは前回の完全送信に成功したstateと比較し、変化したstickだけを含めます。

OSへのpartial writeが途中で失敗した場合はcodecの前回stateを進めません。

firmware側もCRLFまで受信した完全frameだけをstateへcommitします。

不完全な行をtimeoutだけで有効frameとして扱いません。

PokeConの新規connectionではdelta基準をneutralへresetし、最初にneutral frameを送ります。

新規connectionのneutral frameは次のbyte列です。

```text
0x000000 8\r\n
```

接続中にstickがneutral以外だった場合、切断時のneutral frameには必要なstick pairを含みます。

両stickを戻す例は次のとおりです。

```text
0x000003 8 80 80 80 80\r\n
```

firmwareがconnection reset時にlocal stateもneutralへ戻すと、最初の省略frameと同期できます。

## default frameのtest vectorを使う

AとCAPTUREを押し、hatをUpRight、left stickを`(0, 255)`、right stickをneutralにした最初のframeは次のとおりです。

```text
0x008012 1 00 ff\r\n
```

このflagsを分解すると、button部分が`0x008010`、left stick存在bitが`0x000002`です。

同じstateをもう一度送るとstickは変化していないため次のframeになります。

```text
0x008010 1\r\n
```

firmware testでは、2番目のframe後もleft stickが`(0, 255)`のままであることを確認します。

## default parserの最小状態機械を作る

次のpseudocodeはframe境界とdeltaの要点だけを示します。

```text
on_connection_open:
    buttons = 0
    hat = 8
    left = (128, 128)
    right = (128, 128)

on_complete_crlf_line(line):
    tokens = split_ascii_space(line)
    require token_count >= 2
    flags = parse_exact_0x_six_hex(tokens[0])
    next_hat = parse_decimal_u8(tokens[1])
    index = 2

    next_left = left
    next_right = right

    if flags bit 1 is set:
        next_left = parse_two_hex_bytes(tokens[index], tokens[index + 1])
        index += 2

    if flags bit 0 is set:
        next_right = parse_two_hex_bytes(tokens[index], tokens[index + 1])
        index += 2

    require index == token_count
    require next_hat <= 8

    buttons = (flags >> 2) & 0x3fff
    hat = next_hat
    left = next_left
    right = next_right
```

実装では最大行長、無効hex、過剰token、timeout、buffer overflowを閉じて拒否します。

parse error時は直前の完全stateを保持するか、safety方針に従ってneutralへ戻します。

## Qingpi 11-byte frameをparseする

`qingpi`は固定11-byteのbinary frameです。

| offset | size | 値 |
|---:|---:|---|
| 0 | 1 | header `0xab` |
| 1 | 2 | Switch button maskのlittle-endian `u16` |
| 3 | 1 | Switch hat値0から8 |
| 4 | 1 | left stick X |
| 5 | 1 | left stick Y |
| 6 | 1 | constant `128` |
| 7 | 1 | constant `128` |
| 8 | 2 | touch Xのlittle-endian `u16`、未押下時0 |
| 10 | 1 | touch Yの下位byte、未押下時0 |

offset 6と7は常に128であり、現在のencoderはright stickを送信しません。

touch Yのcanonical範囲は0から239なので、一つのbyteに収まります。

touch未押下と座標`(0, 0)`は同じ3-byteの0としてencodeされ、wire上では区別できません。

firmwareで`(0, 0)`のtouchが必要なら、この形式の曖昧性を考慮します。

frameにはlength、checksum、sequenceがありません。

streamから11-byte単位を再構成し、header、hat範囲、constant byteを検証します。

途中でbyteを失った場合のresynchronizationは、`0xab`だけでなくoffset 6と7も利用して誤同期を減らします。

## Qingpiのneutralとtest vectorを使う

neutral frameは次の11 byteです。

```text
ab 00 00 08 80 80 80 80 00 00 00
```

YとCAPTUREを押し、hatをDownLeft、left stickを`(1, 2)`、touchを`(319, 239)`にしたframeは次のとおりです。

```text
ab 01 20 05 01 02 80 80 3f 01 ef
```

button maskのlittle-endian値は`0x2001`です。

touch Xのlittle-endian値は`0x013f`です。

## 3DS 6-byte frameをparseする

`3ds`は固定6-byteのbinary frameです。

| offset | size | 値 |
|---:|---:|---|
| 0 | 1 | header `0xa1` |
| 1 | 1 | 上位nibbleがA、B、X、Y、下位nibbleが3DS hat bits |
| 2 | 1 | bit 0から5がL、R、HOME、PLUS、MINUS、LCLICK |
| 3 | 1 | marker `0xa2` |
| 4 | 1 | 変換済みleft stick X |
| 5 | 1 | 変換済みleft stick Y |

3DS buttonの内部bit割当は次のとおりです。

| 内部bit | button |
|---:|---|
| 0 | A |
| 1 | B |
| 2 | X |
| 3 | Y |
| 4 | L |
| 5 | R |
| 6 | HOME |
| 7 | PLUS |
| 8 | MINUS |
| 9 | LCLICK |

offset 1の上位nibbleは内部bit 0から3を4-bit left shiftした値です。

offset 2は内部button値を4-bit right shiftした後の下位6 bitです。

ZL、ZR、RCLICK、CAPTURE、right stick、touchは3DS frameへencodeしません。

## 3DS hat bitsの制約を扱う

3DS hatの下位nibbleは次の単独directionだけを表します。

| direction | bits |
|---|---:|
| Up | `0x08` |
| Right | `0x04` |
| Down | `0x02` |
| Left | `0x01` |
| diagonal | `0x00` |
| Neutral | `0x00` |

diagonalとneutralはwire上で区別できません。

PokeConはdiagonalを複数bitの組合せへ変換せず、0としてencodeします。

firmwareは0をneutralとして扱うか、使用する3DS側protocolの要件に合わせます。

diagonal入力を必要とする用途にはこの形式が情報を保持しないことを明示します。

## 3DS axis変換をそのまま実装する

left stickの各axisは次の関数で変換します。

```text
if value >= 128:
    encoded = value
else:
    encoded = 127 - value
```

具体例は次のとおりです。

| canonical | encoded |
|---:|---:|
| 0 | 127 |
| 1 | 126 |
| 126 | 1 |
| 127 | 0 |
| 128 | 128 |
| 255 | 255 |

127と128の間に不連続があるため、通常のsigned axis変換へ置き換えません。

firmwareが必要とする最終的なaxis意味は対象3DS adapter側でも確認します。

## 3DSのneutralとtest vectorを使う

neutral frameは次の6 byteです。

```text
a1 00 00 a2 80 80
```

A、HOME、MINUSを押し、hatをLeft、left stickを`(0, 128)`にしたframeは次のとおりです。

```text
a1 11 14 a2 7f 80
```

offset 1の`0x11`はAの上位nibble`0x10`とLeftのhat bit`0x01`です。

offset 2の`0x14`はHOMEとMINUSです。

## connection直後のneutralを受理してから操作する

PokeConはportを開いた直後に、そのformatのneutral frameを完全送信します。

neutral送信に失敗したconnectionはopen成功として公開しません。

firmwareはconnection直後のneutralを通常frameとして受理します。

USB resetやfirmware reset時もlocal controller stateをneutralへ初期化します。

前回connectionのbuttonやstickを不揮発に保持しません。

初期neutralを受信する前に対象consoleへ古い入力を再送しません。

## partial writeとframe interleaveを想定する

OSの一回のwriteがframe全体を受理しなくても、PokeConは残りbyteを同じwrite gateで再送します。

controller frame、`Sender.writeRow()`、`Sender.write()`は同じ非interleave gateを共有します。

一つのframeの途中へ別requestのbyte列を挿入しません。

ただし、連続するbinary frameの間に追加delimiterはありません。

firmwareは固定長またはheader規則で連続frameを分割します。

default text frameはCRLFで分割します。

USB CDCやUARTのread callback一回がframe一個と一致する前提を置きません。

## disconnectと再接続を安全に扱う

明示disconnectでは、queued sendを取り消し、現在formatのneutralを送信し、portを閉じます。

I/O failureではactive connectionを閉じ、明示disconnectでなければ同じ保存設定へ再接続します。

production policyは最大20回です。

最初の試行は直ちに行い、2回目以降は3秒間隔です。

明示disconnectは待機中のretryとqueued sendを取り消します。

再接続で別portを自動選択しません。

firmwareは短時間の再enumerationでも同じprotocol状態を初期化し、最初のneutralから再同期します。

再接続上限へ達した後は、利用者の明示操作で再試行します。

## 設定変更のrollbackを考慮する

接続中にport、baud rate、formatを変更すると、PokeConは旧connectionをneutral化して閉じます。

新設定を正確に開いてneutralを送信できた場合だけ切替をcommitします。

新設定が失敗した場合は、変更前と正確に同じport、baud rate、formatを開き直します。

旧設定への復帰も失敗した場合はdisconnectedになります。

firmware開発中に複数の同型deviceを接続しても、PokeConが別deviceへfailoverすることはありません。

Linuxでは`/dev/serial/by-id`または`/dev/serial/by-path`を使うとdevice node番号の変化を避けやすくなります。

selector文字列の表記は保存したまま保持します。

## peripheralからの受信dataはraw chunkとして扱う

PokeConはserial portから最大4096 byteのbufferで読み、受け取ったchunkをそのままpublishします。

受信側のprotocol decode、line分割、message再構成は行いません。

WebSocketへは`serial.data` eventとしてbase64、元byte長と一緒に送ります。

UIは受信chunkを表示しますが、UIの一行をfirmware message境界として扱いません。

firmwareが応答protocolを提供する場合は、自身のdelimiter、length、checksum、sequenceを定義します。

秘密値や個人情報をserial debug outputへ含めません。

## raw McuCommand rowとの共存を設計する

`McuCommand(sync_name)`は開始時に`sync_name + "\r\n"`を送り、停止時に`end\r\n`を送ります。

`PythonCommand.direct_serial()`と`Sender.writeRow()`もCRLF text rowを送ります。

これらはcontroller formatとは別のraw byte列です。

同じportでcontroller binary frameとtext rowを混在させる場合は、firmware側に曖昧でないmultiplex規則が必要です。

Qingpiの任意byte列や3DS frame内にCRLFと同じbyte patternが現れる可能性を考慮します。

headerだけで識別できないprotocolを同じstreamへ混在させません。

最も安全な設計は、一つの運用modeで一つのframe familyだけを受理することです。

## parserを異常入力で試験する

各formatで少なくとも次のcaseをunit testにします。

- neutral frame
- 全buttonの単独bitと同時押し
- 9個のSwitch hat値
- stickの0、127、128、255
- touchの境界と未押下
- frameを1 byteずつ分割した入力
- 複数frameを一回のreadへ連結した入力
- header直前とframe途中のgarbage
- truncated frameとread timeout
- invalid hatと予約bit
- reconnect後の最初のneutral
- default形式のstick delta省略

Qingpiではtouch `(0, 0)`と未押下の曖昧性を期待値として記録します。

3DSではdiagonalとneutralの曖昧性、未対応button、axis不連続を期待値として記録します。

parser fuzzingではbuffer長に上限を設け、garbage入力で無限待ちやout-of-bounds accessを起こさないことを確認します。

## PTYでnative serial経路を試験する

Linuxではpseudo-terminalのmasterとslaveを使い、物理UARTなしでnative serial backendを通せます。

repositoryの統合taskは次のとおりです。

```bash
nix run .#virtual-io-check
```

このtaskはPTYを実際のdevice nodeとして開き、partial read、partial write、frame非interleave、再接続境界を検査します。

同じtaskはV4L2 loopback cameraも検査します。

実行中kernel用の`v4l2loopback` moduleと、moduleをloadできるpasswordless `sudo`が必要です。

既存のloopback device indexを使用する場合は次のように指定します。

```bash
nix run .#virtual-io-check -- 42
```

PTY testでfirmware parserを接続する場合は、master側でfragmentation、delay、disconnectを注入します。

一回の`write()`が一回のfirmware read callbackになるような単純loopbackだけでは不十分です。

## logic analyzerで実機前のbyte列を確認する

USB serialの場合も可能ならUSB captureまたはMCU UART側でbyte列を取得します。

captureにはPokeCon version、OS、port selectorを秘匿化したidentity、baud rate、data format、操作stateを記録します。

default形式はASCII表示とraw hexの両方を保存します。

binary形式はoffset付きhex dumpとdecode結果を保存します。

initial neutral、操作frame、disconnect neutralの3段階を一つのsequenceとして確認します。

partial frameや重複frameが見つかった場合は、PokeConのwrite境界とfirmwareのread再構成を分けて調べます。

## 対象consoleを含む実機gateを完了する

PTYはOS serial APIまでの回帰を検出できますが、USB抜線、MCU firmware、対象consoleの認識、電気的noiseを再現しません。

release候補ではLinuxとWindowsの両方で実機MCUを使用します。

3形式のframe、button mapping、stick、hat、touch、partial write、抜線、20回再接続、明示disconnect、shutdown neutral、selector rollbackを確認します。

formatが表現しない入力は「成功」として推測せず、制約どおりに欠落することを確認します。

証拠の形式と必須stepは[MCUとシリアルの実機gate](ACCEPTANCE.md#mcuとシリアル)に従います。

実機failureを仮想I/Oの成功だけで合格へ読み替えません。

## 公開protocolと実装の同期を保つ

周辺機器が実装する公開wire契約はこの文書を正本とします。

Rust encoderは`rust/pokecon/src/device/serial/codec.rs`にあります。

button、hat、stick、touchのcanonical stateは`rust/pokecon/src/device/controller.rs`にあります。

connection、partial write、neutral、retry、rollbackは`rust/pokecon/src/device/serial/manager.rs`にあります。

本体開発者がcodecを変更する場合は、test vector、仮想I/O、実機gate、この文書を同じ変更で更新します。

既存firmwareと非互換な変更を単なる内部refactorとして扱いません。
