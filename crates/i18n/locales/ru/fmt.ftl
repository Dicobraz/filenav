# Размеры и даты (urfin_i18n::fmt). Разделители дробной части и разрядов
# задаёт язык (fmt::NumFmt), а не этот файл.
fmt-unit-b = Б
fmt-unit-kb = КБ
fmt-unit-mb = МБ
fmt-unit-gb = ГБ
fmt-unit-tb = ТБ
fmt-unit-pb = ПБ
fmt-unit-eb = ЭБ
fmt-datetime = %d.%m.%y %H:%M
# $size — точный размер с разрядами, $count — то же число для плюрали.
fmt-bytes-exact = { $size } { $count ->
    [one] байт
    [few] байта
   *[many] байт
}
