# Sizes and dates (urfin_i18n::fmt). Decimal and group separators come from
# the language (fmt::NumFmt), not from here.
fmt-unit-b = B
fmt-unit-kb = KB
fmt-unit-mb = MB
fmt-unit-gb = GB
fmt-unit-tb = TB
fmt-unit-pb = PB
fmt-unit-eb = EB
# jiff strftime pattern.
fmt-datetime = %m/%d/%y %-I:%M %p
# $size — exact size with group separators, $count — the same number for the plural rule.
fmt-bytes-exact = { $size } { $count ->
    [one] byte
   *[other] bytes
}
