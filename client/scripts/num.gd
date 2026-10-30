## 定点数：服务器的数值一律是整数，带小数的量以千分之一为单位（1.5 是 1500）。
## 显示和输入时才换算，全用整数运算。只有静态函数，不依赖别的脚本，可以单独测
extends RefCounted

const FX := 1000


## 四舍五入的整数除法
static func div_round(n: int, d: int) -> int:
	if d == 0:
		return 0
	var q := absi(n) * 2 + absi(d)
	var r := q / (absi(d) * 2)
	return r if (n < 0) == (d < 0) else -r


## 定点数显示成小数：100 以上取整，否则保留一位，整数不带小数点
static func fx(x) -> String:
	var raw := int(x)
	if absi(raw) >= 100 * FX:
		return str(div_round(raw, FX))
	var tenths := div_round(raw, FX / 10)
	if tenths % 10 == 0:
		return str(tenths / 10)
	return "%s%d.%d" % ["-" if tenths < 0 else "", absi(tenths) / 10, absi(tenths) % 10]


## 输入框里写的小数换成定点数：「1.5」→ 1500。最多三位小数，看不懂或负数返回 -1
static func parse_fx(text: String) -> int:
	var t := text.strip_edges()
	if t == "":
		return 0
	var parts := t.split(".")
	if parts.size() > 2 or not parts[0].is_valid_int() or parts[0].begins_with("-") or parts[0].begins_with("+"):
		return -1
	var frac := parts[1] if parts.size() == 2 else ""
	if frac.length() > 3 or (frac != "" and not frac.is_valid_int()) or frac.begins_with("-") or frac.begins_with("+"):
		return -1
	return int(parts[0]) * FX + int(frac.rpad(3, "0"))


## 定点数放进输入框：1500 →「1.5」
static func fx_input(x) -> String:
	var raw := int(x)
	var frac := str(raw % FX).pad_zeros(3).rstrip("0")
	return str(raw / FX) if frac == "" else "%d.%s" % [raw / FX, frac]


## 定点数的比例显示成百分数（整数）：250 → 25
static func pct(x) -> int:
	return div_round(int(x), FX / 100)
