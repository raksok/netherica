1. 閉規格餘隙
- [x] 修 `Netherica_rqrmnt.md` — 凡產品級剩餘改為 `product_id + department_id` 範疇
- [x] 改6.2節範例，用部門感知查詢如 `sum_ledger_for_product_department_before_date(product_id, department_id, cutoff_date)`
- [x] 明定 `factor == 1` 規則：不追蹤期初/期末剩餘，乾運行及報表列皆視為 0
- [x] 修 Review 視圖用語：`Total Subunits Used` 顯式為 `Product + Department` 每對，列數指產品-部門列
- [x] 修 Report 節用語：非零結轉剩餘之列，即使當前檔案無該部門交易亦須出現
- [x] 擴充需求測試節：涵蓋部門範疇剩餘、`factor == 1`、結轉報表列、遷移/回填行為

2. 資料庫遷移與查詢效能
- [x] 增遷移 `v1 -> v2`：`product_totals` 主鍵改 `(product_id, department_id)`
- [x] 回填新 `product_totals`：自 `inventory_ledger` 按 `product_id, department_id` 分組聚合
- [x] 註冊遷移於啟動載入，相應更新 `PRAGMA user_version` 處理
- [x] 增索引支援新截止查詢模式
- [x] 複合索引 `(product_id, department_id, transaction_date)` 為佳（乾運行與報表期初剩餘皆部門範疇）
- [x] 驗證遷移運行於交易內，失敗不留部分狀態

3. 儲存庫合約更新
- [x] 更新儲存庫型別與 SQL：`product_totals` upsert 必含 `department_id`
- [x] 取代僅產品累積和查詢為部門感知查詢（截止計算）
- [x] 增儲存庫查詢：報表生成返回所有 `Product + Department` 列，其結轉剩餘非零（相關截止/檔案上下文）
- [x] 更新當前總量查詢輔助：使用複合鍵或返回分組列，非單一產品總量

4. 領域模型與聚合變更
- [x] 更新乾運行/領域列結構：主調節單元為一 `Product + Department` 列
- [x] 匯入檔案交易按 `product_id + department_id` 分組後計算乾運行度量
- [x] 計算期初剩餘：同一 `product_id + department_id` 至匯入檔案最末日前所有帳本交易
- [x] 僅當 `factor != 1` 保留歐幾里得模邏輯
- [x] 計算整數產出：用既有公式，基於部門範疇前期與新總量
- [x] 保留部門映射/顯示名附於每列，使 UI 與報表可渲染穩定標籤

5. 攝取與提交路徑
- [x] 驗證 Excel 解析保留 `department_id`（欄10）貫穿全攝取管線
- [x] 更新工作流層乾運行準備：僅新檔案中出現之 `Product + Department` 組合顯示於 Review 視圖
- [x] 更新提交時聚合：`product_totals` 每 `Product + Department` 組增量一次，於同一 ACID 交易內
- [x] 確保零數量列仍跳過，不建立空產品-部門組

6. Review 視圖更新
- [x] 改乾運行表格：每列為新檔案中一 `Product + Department`
- [x] 渲染欄位：`Product`、`Department`、`Opening Leftover`、`Total Subunits Used`、`Whole Units Output`、`Closing Leftover`
- [x] 一致顯示映射部門標籤，原始碼按現 UI 模式需求可得
- [x] 更新列數文字與摘要文字：指調節列而非產品
- [x] 確認 `factor == 1` 列顯示期初/期末剩餘為 0，仍正確顯示用量/產出值

7. 報表生成更新
- [x] 更新報表資料建構器：部門列為一等報表項目
- [x] 包含所有結轉期初剩餘非零列，即使新匯入檔案無該部門交易
- [x] 保持確定性排序（先產品後部門），跨生成一致
- [x] 確保報表模板匹配樣本列印佈局，同時使用部門範疇期初剩餘值
- [x] 驗證重新生成報表使用與提交後報表相同之部門範疇邏輯

8. 回歸與驗證
- [x] 增單元測試：部門範疇模行為、`factor == 1` 無剩餘路徑
- [x] 增整合測試：從現有 v1 資料庫遷移回填
- [x] 增整合測試：同一產品跨多部門、跨多檔案
- [x] 增整合測試：Review 視圖僅顯示新檔案中存在之產品-部門列
- [x] 增整合測試：報表包含結轉剩餘非零列，即使當前檔案無該部門交易
- [x] 重跑現有重複檔案與時序測試，確認無回歸
- [x] 執行 Rust 專案完整驗證命令集
