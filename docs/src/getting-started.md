# 起動と最初の操作

## 準備

ソースから実行する場合はRustと、Bevy/wgpuに対応するグラフィックス環境が必要です。ソルバーを動かす場合はFrontISTRを別途用意します。既存結果を表示するだけなら、ソルバーの実行は不要です。

リポジトリのルートで実行します。

```text
cargo run --package bevyistr
```

処理性能を確認するときはreleaseビルドを使用できます。

```text
cargo run --release --package bevyistr
```

## まず結果を表示する

1. **Results**ページへ移動します。
2. **Open Results...**でASCII VTU／PVTUを選びます。
3. **Display field**で表示する物理量を選びます。
4. [カメラ操作](operation.md)でモデルを回転し、凡例と値を確認します。

VTU／PVTUは形状を含むため、MSHを先に開く必要はありません。RESの場合は対応するMSHを案内します。詳細は[結果を開く](results/opening.md)を参照してください。

## 解析設定から始める

1. **Model → Open Project**で`hecmw_ctrl.dat`を選びます。
2. 材料、拘束・荷重、接触など、読み込まれた設定を各ページで確認します。
3. **Solve**で出力先と実行設定を確認します。

元のプロジェクトは別に保管してください。読み込み・編集・再出力はFrontISTRの全キーワードを保証するものではありません。
