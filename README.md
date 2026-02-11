## 概要
[![Release](https://img.shields.io/github/v/release/clean262/sam_frame_export_filter)](https://github.com/clean262/sam_frame_export_filter)
[![Downloads](https://img.shields.io/github/downloads/clean262/sam_frame_export_filter/total)](https://github.com/clean262/sam_frame_export_filter/releases/latest)
[![License](https://img.shields.io/github/license/clean262/sam_frame_export_filter?v=2)](https://github.com/clean262/sam_frame_export_filter/blob/main/LICENSE)
[![Last Commit](https://img.shields.io/github/last-commit/clean262/sam_frame_export_filter)](https://github.com/clean262/sam_frame_export_filter/commits/main)

クリックベースで簡単に物体を切り取れるAviutl2プラグインです。
MetaのSAMを用いています。

現状**静止画のみ**に対応しています。(動画素材の場合は静止画に切り取ってから背景除去を行います。)

**動画素材に対して**BB素材として切り取るには[こちら](https://www.nicovideo.jp/watch/sm44970717) (注: Aviutl2プラグインではありません)

- マウスクリックだけで前景・背景を指定
- ブラウザ上でプレビューしながらマスクを調整
- 複数の SAM モデル（軽量〜高品質）から選択可能

![use example](assets/example.png?raw=true)

## 利用に際して
本スクリプトを用いて動画制作を行った場合親作品登録をしていただけると喜びます。

親作品登録いただいたら見に行きます。

**解説動画**は[こちら](https://www.nicovideo.jp/watch/sm45723074)から

将来的には動画素材にも対応できるように検討中です。開発状況等は作者[X: 旧twitter](https://x.com/clean123525)を参照してください。

バグ報告や機能追加の要望がありましたら[Issues](https://github.com/clean262/sam_frame_export_filter/issues)から気軽にお願いします。
右上`New issue`ボタンから送れます。

## 導入方法
### 1. webGPUを使えるようにする
`chrome://gpu` とGoogle Chromeで入力し `WebGPU: Hardware accelerated` があればOK

ない場合は`chrome://settings/system`を入力し、

「グラフィック アクセラレーションが使用可能な場合は使用する」をオンにしChromeを再起動してください

![check web gpu usage](assets/webgpu.png?raw=true)

### 2. Releaseからダウンロードする
[Release](https://github.com/clean262/sam_frame_export_filter/releases/latest) から zipファイルをダウンロードして下さい。

### 3. zipファイルを展開し、中身を配置する
zipファイルを展開し、中身を以下のように配置してください

|ファイル・フォルダ名|配置先|
|:---|:---|
|`sam_frame_export_filter.auf2`<br>`sam_frame_export_filter`フォルダ|`C:\ProgramData\aviutl2\Plugin`フォルダ|

最終的に以下の構成になれば成功です。

```bash
C:\ProgramData\aviutl2\Plugin\
  ├─ sam_frame_export_filter.auf
  └─ sam_frame_export_filter\
       └─ web\
           ├─ index.html
           ├─ index.css
           └─ index.js
```

## 使い方
### 1. フィルタ効果としてSAM Frame Export(PNG)を選ぶ
デフォルトで抽出のカテゴリに入っています

![select filter](assets/select.png?raw=true)

### 2. 保存するフォルダを選ぶ
デフォルトで切り抜いた結果は`C:\ProgramData\aviutl2\Export`に保存されます。

`保存先フォルダ内`を選択すると、どのフォルダに保存するかを指定できます。

選択したファイルと同じ場所に結果が保存されるようになります。

cf) `C:\ProgramData\aviutl2\example\a.png`を選択した場合`C:\ProgramData\aviutl2\example\`に保存されます。

![extract png](assets/filter.png?raw=true)

### 3. 抽出したいタイミングに合わせる
切り抜きたいタイミングに合わせて

✅ このフレームをSAMで前景抽出

にチェックを入れてください。

切り抜きたいタイミングを間違えた場合はチェックを外してもう一度入れなおすと変更が出来ます。

### 4. 切り抜きたい物体を選択する
自動的にブラウザ上で切り抜き用のページが開きます。誤って閉じた場合や開かない場合はブラウザ上で直接 `http://127.0.0.1:17860/`を開いて下さい。

ブラウザページ上の「SAM: Model」から3つのモデルが選択できます
1. slimsam-77-uniform: 最も早いが性能の悪いモデル。本Document冒頭の犬なら容易に切り抜けますがアニメ素材などでは弱いです
2. sam-vit-base: 間のモデル
3. sam-vit-large: 最も遅いが性能の良いモデル

モデルの性能差の例は[モデルの違い](#モデルの違い)の項を参照してください。

`Load from Aviutl2`を押すと先ほど選択したシーンが表示されます。

左クリックで切り抜きたい部分、右クリックで取り除きたい部分を選択できます。

選択する点を誤った場合`Clear points`を押してください。

選択終了後は`Cut masks`を押してください。

結果が`C:\ProgramData\aviutl2\Export`ないしあなたが選択したフォルダに保存されます。

![extract png](assets/web_app.png?raw=true)

### 5. 切り抜いた物体をDrag and drop
切り抜いた画像をタイムライン上に挿入してください。

## モデルの違い
最も軽量だが性能の悪い`slimsam-77-uniform`で切り抜いた結果が以下になります
冒頭の犬はこちらのモデルで切り抜いたものです。画像に合わせて適切なモデルを選択してください。
![bad result](assets/small_model.png?raw=true)

一方最も遅いが性能の良い`sam-vit-large`で切り抜いた結果が以下になります

![bad result](assets/large_model.png?raw=true)

## ライセンス

**MIT ライセンス**です。