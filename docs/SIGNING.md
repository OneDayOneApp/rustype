# Rustype 正式签名与公证

## 0. 当前状态

- [x] 已生成证书请求（CSR）：`~/.rustype-signing/developerID.certSigningRequest`
- [x] 私钥：`~/.rustype-signing/developerID.key`（600，务必备份）
- [ ] 加入 Apple Developer Program（$99/年）
- [ ] 在开发者后台创建 Developer ID Application 证书
- [ ] 下载 `.cer` 并导入钥匙串
- [ ] 用 Developer ID 重新打包
- [ ] 公证并装订

## 1. 加入开发者计划

<https://developer.apple.com/programs/enroll/>

- 个人：Apple ID + 双重认证 + 证件核验，通常 1–3 天；
- 公司：另需 D-U-N-S 编号，1–2 周。

## 2. 创建 Developer ID Application 证书

1. 打开 <https://developer.apple.com/account/resources/certificates/list>；
2. `+` → **Developer ID Application** → 上传
   `~/.rustype-signing/developerID.certSigningRequest`；
3. 下载 `.cer`，放到 `~/Downloads`。

## 3. 导入证书到钥匙串

```bash
cd ~/.rustype-signing
openssl x509 -inform DER -in ~/Downloads/developerID_application.cer -out developerID.pem
openssl pkcs12 -export -inkey developerID.key -in developerID.pem \
  -out developerID.p12 -name "Developer ID Application"
security import developerID.p12 -k ~/Library/Keychains/login.keychain-db \
  -P "设置一个临时密码" -T /usr/bin/codesign
security find-identity -v -p codesigning
```

第一次 `codesign` 可能会弹窗询问是否允许使用私钥，选择「始终允许」。

## 4. 用 Developer ID 打包

```bash
export SIGN_ID="Developer ID Application: Your Name (TEAMID)"
RUSTYPE_BIN=target/universal/rustype scripts/make-app.sh
RUSTYPE_BIN=target/universal/rustype scripts/make-dmg.sh 0.1.0-universal
codesign --verify --deep --strict dist/Rustype.app
```

## 5. 保存公证凭据

```bash
xcrun notarytool store-credentials rustype \
  --apple-id "you@example.com" \
  --team-id "TEAMID" \
  --password "app-specific-password"   # appleid.apple.com 生成
```

## 6. 公证 + 装订

```bash
SIGN_ID="$SIGN_ID" scripts/notarize.sh dist/Rustype-0.1.0-universal.dmg
```

## 7. 验证

```bash
spctl -a -vvv -t install dist/Rustype-0.1.0-universal.dmg
xcrun stapler validate dist/Rustype-0.1.0-universal.dmg
```
