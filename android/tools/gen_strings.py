"""Generate Android string resources (10 languages) from the phone page dictionaries in web/src/i18n/*.ts
plus the Android-only strings below.  Run from anywhere:  python android/tools/gen_strings.py
The generated files are committed, so Python is only needed when a text changes."""
import io, os, re

ROOT = os.path.normpath(os.path.join(os.path.dirname(__file__), '..', '..'))
WEB = os.path.join(ROOT, 'web', 'src', 'i18n')
RES = os.path.join(ROOT, 'android', 'app', 'src', 'main', 'res')

LANGS = {  # web code -> Android values folder
    'en': 'values', 'vi': 'values-vi', 'zh': 'values-zh', 'ja': 'values-ja', 'ko': 'values-ko',
    'id': 'values-in', 'es': 'values-es', 'fr': 'values-fr', 'de': 'values-de', 'ru': 'values-ru',
}

# Android-only texts. Order of each tuple follows LANGS.
EXTRA = {
    'notif.title': ('LarmaMic is streaming', 'LarmaMic đang truyền tiếng', 'LarmaMic 正在传送声音', 'LarmaMic が送信中です',
                    'LarmaMic이 전송 중입니다', 'LarmaMic sedang mengirim suara', 'LarmaMic está transmitiendo',
                    'LarmaMic diffuse le son', 'LarmaMic überträgt', 'LarmaMic передаёт звук'),
    'notif.text': ('Tap to open. The microphone keeps working with the screen off.', 'Chạm để mở. Mic vẫn hoạt động khi tắt màn hình.',
                   '点按打开。关闭屏幕后麦克风仍会工作。', 'タップして開きます。画面を消してもマイクは動作します。',
                   '탭하여 열기. 화면이 꺼져도 마이크는 계속 작동합니다.', 'Ketuk untuk membuka. Mikrofon tetap bekerja saat layar mati.',
                   'Toca para abrir. El micrófono sigue funcionando con la pantalla apagada.', 'Touchez pour ouvrir. Le micro continue de fonctionner écran éteint.',
                   'Zum Öffnen tippen. Das Mikrofon läuft auch bei ausgeschaltetem Bildschirm weiter.',
                   'Нажмите, чтобы открыть. Микрофон работает и при выключенном экране.'),
    'notif.stop': ('Stop', 'Dừng', '停止', '停止', '중지', 'Berhenti', 'Detener', 'Arrêter', 'Stopp', 'Стоп'),
    'scan.btn': ('Scan QR code', 'Quét mã QR', '扫描二维码', 'QR コードをスキャン', 'QR 코드 스캔', 'Pindai kode QR', 'Escanear código QR',
                 'Scanner le QR code', 'QR-Code scannen', 'Сканировать QR-код'),
    'scan.prompt': ('Scan the QR code shown in LarmaMic on your PC', 'Quét mã QR hiện trong LarmaMic trên PC',
                    '扫描电脑上 LarmaMic 显示的二维码', 'PC の LarmaMic に表示された QR コードをスキャンしてください',
                    'PC의 LarmaMic에 표시된 QR 코드를 스캔하세요', 'Pindai kode QR yang tampil di LarmaMic pada PC',
                    'Escanea el código QR que muestra LarmaMic en tu PC', 'Scannez le QR code affiché par LarmaMic sur votre PC',
                    'Scanne den QR-Code, den LarmaMic auf dem PC anzeigt', 'Отсканируйте QR-код, показанный в LarmaMic на ПК'),
    'addr.label': ('Or paste the address', 'Hoặc dán địa chỉ', '或粘贴地址', 'またはアドレスを貼り付け', '또는 주소 붙여넣기', 'Atau tempel alamat',
                   'O pega la dirección', 'Ou collez l’adresse', 'Oder Adresse einfügen', 'Или вставьте адрес'),
    'addr.connect': ('Connect', 'Kết nối', '连接', '接続', '연결', 'Hubungkan', 'Conectar', 'Se connecter', 'Verbinden', 'Подключить'),
    'addr.invalid': ('That address is not valid.', 'Địa chỉ không hợp lệ.', '地址无效。', 'アドレスが正しくありません。', '주소가 올바르지 않습니다.',
                     'Alamat itu tidak valid.', 'Esa dirección no es válida.', 'Cette adresse n’est pas valide.', 'Diese Adresse ist ungültig.',
                     'Недопустимый адрес.'),
    'conn.last': ('Paired PC: {0}', 'PC đã ghép nối: {0}', '已配对的电脑：{0}', 'ペアリング済みの PC：{0}', '페어링된 PC: {0}', 'PC terpasang: {0}',
                  'PC emparejado: {0}', 'PC appairé : {0}', 'Gekoppelter PC: {0}', 'Сопряжённый ПК: {0}'),
    'conn.forget': ('Forget this PC', 'Quên PC này', '忘记这台电脑', 'この PC を削除', '이 PC 잊기', 'Lupakan PC ini', 'Olvidar este PC',
                    'Oublier ce PC', 'Diesen PC vergessen', 'Забыть этот ПК'),
    'perm.mic': ('Microphone permission is needed to stream your voice.', 'Cần quyền dùng micro để truyền giọng nói.',
                 '需要麦克风权限才能传送你的声音。', '声を送信するにはマイクの権限が必要です。', '목소리를 전송하려면 마이크 권한이 필요합니다.',
                 'Izin mikrofon diperlukan untuk mengirim suara Anda.', 'Se necesita permiso de micrófono para transmitir tu voz.',
                 'L’autorisation du micro est nécessaire pour diffuser votre voix.', 'Für die Übertragung deiner Stimme wird die Mikrofonberechtigung benötigt.',
                 'Для передачи голоса нужно разрешение на микрофон.'),
    'err.net': ('Could not reach the PC. Check that both are on the same Wi-Fi and LarmaMic is open.',
                'Không nối được tới PC. Kiểm tra cả hai cùng Wi-Fi và app LarmaMic đang mở.',
                '无法连接电脑。请确认两者在同一 Wi-Fi 且 LarmaMic 已打开。', 'PC に接続できません。同じ Wi-Fi にあり、LarmaMic が開いているか確認してください。',
                'PC에 연결할 수 없습니다. 같은 Wi-Fi에 있고 LarmaMic이 열려 있는지 확인하세요.',
                'Tidak dapat terhubung ke PC. Pastikan keduanya di Wi-Fi yang sama dan LarmaMic terbuka.',
                'No se pudo conectar con el PC. Comprueba que ambos estén en la misma Wi-Fi y que LarmaMic esté abierto.',
                'Impossible de joindre le PC. Vérifiez qu’ils sont sur le même Wi-Fi et que LarmaMic est ouvert.',
                'PC nicht erreichbar. Prüfe, ob beide im selben WLAN sind und LarmaMic geöffnet ist.',
                'Не удалось подключиться к ПК. Проверьте, что оба в одной сети Wi-Fi и LarmaMic открыт.'),
    'err.cert': ('The PC’s security certificate changed. Scan the QR code again to trust it.',
                 'Chứng chỉ bảo mật của PC đã đổi. Quét lại mã QR để tin cậy lại.',
                 '电脑的安全证书已变化。请重新扫描二维码以信任它。', 'PC のセキュリティ証明書が変わりました。QR コードをもう一度スキャンして信頼してください。',
                 'PC의 보안 인증서가 변경되었습니다. QR 코드를 다시 스캔하여 신뢰하세요.',
                 'Sertifikat keamanan PC berubah. Pindai kode QR lagi untuk mempercayainya.',
                 'El certificado de seguridad del PC cambió. Vuelve a escanear el código QR para confiar en él.',
                 'Le certificat de sécurité du PC a changé. Scannez à nouveau le QR code pour lui faire confiance.',
                 'Das Sicherheitszertifikat des PCs hat sich geändert. Scanne den QR-Code erneut, um ihm zu vertrauen.',
                 'Сертификат безопасности ПК изменился. Отсканируйте QR-код ещё раз, чтобы доверять ему.'),
    'mode.title': ('Connection', 'Kết nối', '连接方式', '接続方法', '연결 방식', 'Koneksi', 'Conexión', 'Connexion', 'Verbindung', 'Подключение'),
    'mode.usb': ('USB cable', 'Cáp USB', 'USB 数据线', 'USB ケーブル', 'USB 케이블', 'Kabel USB', 'Cable USB', 'Câble USB', 'USB-Kabel', 'USB-кабель'),
    'mode.wifi.hint': ('The phone and the PC must be on the same Wi-Fi.', 'Điện thoại và PC phải cùng Wi-Fi.', '手机和电脑必须在同一 Wi-Fi 下。',
                       'スマートフォンと PC が同じ Wi-Fi に接続されている必要があります。', '휴대폰과 PC가 같은 Wi-Fi에 있어야 합니다.',
                       'Ponsel dan PC harus berada di Wi-Fi yang sama.', 'El teléfono y el PC deben estar en la misma Wi-Fi.',
                       'Le téléphone et le PC doivent être sur le même Wi-Fi.', 'Handy und PC müssen im selben WLAN sein.',
                       'Телефон и ПК должны быть в одной сети Wi-Fi.'),
    'mode.usb.hint': (
        'Plug the cable into the PC, then either turn on USB debugging on the phone (detected automatically) or turn on USB tethering (Network > Hotspot and tethering). No Wi-Fi needed.',
        'Cắm cáp vào PC, rồi bật Gỡ lỗi USB trên điện thoại (tự nhận) hoặc bật Chia sẻ kết nối qua USB (Mạng > Điểm phát sóng và chia sẻ). Không cần Wi-Fi.',
        '把数据线接到电脑，然后在手机上开启 USB 调试（自动识别），或开启 USB 网络共享（网络 > 热点和网络共享）。无需 Wi-Fi。',
        'ケーブルを PC に接続し、スマートフォンで USB デバッグをオン（自動認識）にするか、USB テザリング（ネットワーク > アクセスポイントとテザリング）をオンにします。Wi-Fi は不要です。',
        '케이블을 PC에 연결한 다음 휴대폰에서 USB 디버깅을 켜거나(자동 인식) USB 테더링을 켜세요(네트워크 > 핫스팟 및 테더링). Wi-Fi는 필요 없습니다.',
        'Colokkan kabel ke PC, lalu aktifkan Debugging USB di ponsel (terdeteksi otomatis) atau aktifkan tethering USB (Jaringan > Hotspot dan tethering). Tidak perlu Wi-Fi.',
        'Conecta el cable al PC y luego activa la depuración USB en el teléfono (se detecta sola) o la conexión USB compartida (Red > Punto de acceso y conexión compartida). No hace falta Wi-Fi.',
        'Branchez le câble au PC, puis activez le débogage USB sur le téléphone (détection automatique) ou le partage de connexion USB (Réseau > Point d’accès et partage). Pas besoin de Wi-Fi.',
        'Kabel am PC anschließen, dann am Handy USB-Debugging aktivieren (wird automatisch erkannt) oder USB-Tethering (Netzwerk > Hotspot und Tethering). Kein WLAN nötig.',
        'Подключите кабель к ПК, затем включите на телефоне отладку по USB (определится автоматически) или USB-модем (Сеть > Точка доступа и модем). Wi-Fi не нужен.',
    ),
    'mode.bt.hint': ('Pair the phone with the PC in Bluetooth settings, turn on Bluetooth tethering on the phone and join its network on the PC, then scan the QR code. Slower than Wi-Fi: audio is sent at 24 kHz.',
                     'Ghép đôi điện thoại với PC trong cài đặt Bluetooth, bật chia sẻ kết nối qua Bluetooth trên điện thoại và vào mạng đó trên PC, rồi quét mã QR. Chậm hơn Wi-Fi: âm thanh gửi ở 24 kHz.',
                     '在蓝牙设置中配对手机和电脑，在手机上开启蓝牙网络共享并在电脑上加入该网络，然后扫描二维码。比 Wi-Fi 慢：音频以 24 kHz 发送。',
                     'Bluetooth 設定でスマートフォンと PC をペアリングし、スマートフォンで Bluetooth テザリングをオンにして PC でそのネットワークに参加し、QR コードをスキャンします。Wi-Fi より遅く、音声は 24 kHz で送信されます。',
                     '블루투스 설정에서 휴대폰과 PC를 페어링하고, 휴대폰에서 블루투스 테더링을 켜고 PC에서 해당 네트워크에 연결한 다음 QR 코드를 스캔하세요. Wi-Fi보다 느리며 오디오는 24 kHz로 전송됩니다.',
                     'Pasangkan ponsel dengan PC di pengaturan Bluetooth, aktifkan tethering Bluetooth di ponsel dan gabung ke jaringannya di PC, lalu pindai kode QR. Lebih lambat dari Wi-Fi: audio dikirim pada 24 kHz.',
                     'Empareja el teléfono con el PC en los ajustes de Bluetooth, activa la conexión compartida por Bluetooth en el teléfono, únete a su red en el PC y escanea el código QR. Más lento que Wi-Fi: el audio se envía a 24 kHz.',
                     'Appairez le téléphone au PC dans les réglages Bluetooth, activez le partage de connexion Bluetooth sur le téléphone et rejoignez son réseau sur le PC, puis scannez le QR code. Plus lent que le Wi-Fi : l’audio est envoyé à 24 kHz.',
                     'Handy und PC in den Bluetooth-Einstellungen koppeln, auf dem Handy Bluetooth-Tethering aktivieren, am PC dessen Netzwerk beitreten und den QR-Code scannen. Langsamer als WLAN: Audio wird mit 24 kHz gesendet.',
                     'Сопрягите телефон с ПК в настройках Bluetooth, включите Bluetooth-модем на телефоне, подключитесь к его сети на ПК и отсканируйте QR-код. Медленнее Wi-Fi: звук передаётся на 24 кГц.'),
    'notif.mute': (
        'Mute',
        'Tắt tiếng',
        '静音',
        'ミュート',
        '음소거',
        'Bisukan',
        'Silenciar',
        'Couper le son',
        'Stumm',
        'Выкл. звук',
    ),
    'notif.unmute': (
        'Unmute',
        'Bật tiếng',
        '取消静音',
        'ミュート解除',
        '음소거 해제',
        'Aktifkan suara',
        'Activar sonido',
        'Réactiver le son',
        'Ton an',
        'Вкл. звук',
    ),
    'vol.title': (
        'Volume keys mute the mic',
        'Phím âm lượng tắt/bật tiếng mic',
        '音量键切换麦克风静音',
        '音量キーでマイクをミュート',
        '볼륨 키로 마이크 음소거',
        'Tombol volume membisukan mikrofon',
        'Las teclas de volumen silencian el micrófono',
        'Les touches de volume coupent le micro',
        'Lautstärketasten schalten das Mikrofon stumm',
        'Кнопки громкости отключают микрофон',
    ),
    'vol.hint': (
        'Press a volume button to mute or unmute, even with the screen off. Takes effect the next time you start.',
        'Nhấn nút âm lượng để tắt/bật tiếng, kể cả khi tắt màn hình. Có hiệu lực từ lần bật sau.',
        '按音量键即可静音或取消静音，息屏时也有效。下次启动时生效。',
        '音量ボタンでミュート／解除できます。画面が消えていても有効です。次回の開始から反映されます。',
        '볼륨 버튼으로 음소거/해제합니다. 화면이 꺼져도 작동합니다. 다음 시작부터 적용됩니다.',
        'Tekan tombol volume untuk membisukan atau mengaktifkan suara, bahkan saat layar mati. Berlaku mulai pengaktifan berikutnya.',
        'Pulsa un botón de volumen para silenciar o activar, incluso con la pantalla apagada. Se aplica en el próximo inicio.',
        'Appuyez sur une touche de volume pour couper ou réactiver le son, même écran éteint. Pris en compte au prochain démarrage.',
        'Lautstärketaste drücken, um stumm zu schalten oder aufzuheben, auch bei ausgeschaltetem Bildschirm. Gilt ab dem nächsten Start.',
        'Нажмите кнопку громкости, чтобы отключить или включить звук, даже при выключенном экране. Вступает в силу при следующем запуске.',
    ),
    'tile.label': (
        'Mic mute',
        'Tắt tiếng mic',
        '麦克风静音',
        'マイクをミュート',
        '마이크 음소거',
        'Bisukan mikrofon',
        'Silenciar micrófono',
        'Couper le micro',
        'Mikrofon stumm',
        'Микрофон выкл.',
    ),
    'nearby.title': (
        'Nearby PCs',
        'PC gần đây',
        '附近的电脑',
        '近くの PC',
        '근처 PC',
        'PC di sekitar',
        'PC cercanos',
        'PC à proximité',
        'PCs in der Nähe',
        'ПК поблизости',
    ),
    'nearby.none': (
        'Searching for PCs on this network…',
        'Đang tìm PC trong mạng này…',
        '正在此网络中搜索电脑…',
        'このネットワーク上の PC を検索中…',
        '이 네트워크에서 PC를 찾는 중…',
        'Mencari PC di jaringan ini…',
        'Buscando PC en esta red…',
        'Recherche de PC sur ce réseau…',
        'Suche nach PCs in diesem Netzwerk…',
        'Поиск ПК в этой сети…',
    ),
    'nearby.wait': (
        'Waiting for you to allow this phone on “{0}”…',
        'Đang chờ bạn cho phép điện thoại này trên “{0}”…',
        '正在等待你在“{0}”上允许此手机…',
        '「{0}」でこのスマートフォンを許可するのを待っています…',
        '“{0}”에서 이 휴대폰을 허용하기를 기다리는 중…',
        'Menunggu Anda mengizinkan ponsel ini di “{0}”…',
        'Esperando a que permitas este teléfono en «{0}»…',
        'En attente de votre autorisation pour ce téléphone sur « {0} »…',
        'Warte darauf, dass du dieses Handy auf „{0}“ zulässt…',
        'Ожидание: разрешите этот телефон на «{0}»…',
    ),
    'nearby.refused': (
        'The PC refused or did not answer in time.',
        'PC đã từ chối hoặc không trả lời kịp.',
        '电脑已拒绝或未及时回应。',
        'PC が拒否したか、時間内に応答しませんでした。',
        'PC가 거부했거나 제때 응답하지 않았습니다.',
        'PC menolak atau tidak menjawab tepat waktu.',
        'El PC lo rechazó o no respondió a tiempo.',
        'Le PC a refusé ou n’a pas répondu à temps.',
        'Der PC hat abgelehnt oder nicht rechtzeitig geantwortet.',
        'ПК отклонил запрос или не ответил вовремя.',
    ),
    'hear.title': (
        'Hear myself (headphones)',
        'Nghe lại giọng mình (tai nghe)',
        '监听自己的声音（耳机）',
        '自分の声をモニター（ヘッドホン）',
        '내 목소리 듣기(헤드폰)',
        'Dengar suara sendiri (headphone)',
        'Escucharme (auriculares)',
        'M’entendre (casque)',
        'Mich selbst hören (Kopfhörer)',
        'Слышать себя (наушники)',
    ),
    'hear.hint': (
        'Plays your processed voice back on this phone. Use headphones, otherwise you will get feedback. Not available over Bluetooth.',
        'Phát lại giọng đã xử lý trên điện thoại này. Hãy đeo tai nghe, nếu không sẽ bị hú. Không dùng được qua Bluetooth.',
        '在这部手机上回放处理后的声音。请使用耳机，否则会产生啸叫。蓝牙模式下不可用。',
        '処理後の声をこのスマートフォンで再生します。ヘッドホンを使わないとハウリングします。Bluetooth では使えません。',
        '처리된 목소리를 이 휴대폰에서 다시 재생합니다. 헤드폰을 사용하지 않으면 하울링이 생깁니다. 블루투스에서는 사용할 수 없습니다.',
        'Memutar kembali suara yang sudah diproses di ponsel ini. Gunakan headphone agar tidak berdengung. Tidak tersedia lewat Bluetooth.',
        'Reproduce tu voz procesada en este teléfono. Usa auriculares o habrá acople. No disponible por Bluetooth.',
        'Rejoue votre voix traitée sur ce téléphone. Utilisez un casque, sinon vous aurez un larsen. Indisponible en Bluetooth.',
        'Spielt deine bearbeitete Stimme auf diesem Handy ab. Nutze Kopfhörer, sonst gibt es Rückkopplung. Über Bluetooth nicht verfügbar.',
        'Воспроизводит обработанный голос на этом телефоне. Используйте наушники, иначе будет свист. Недоступно по Bluetooth.',
    ),
    'err.addr': ('Scan the QR code first, or paste the address shown in LarmaMic.',
                 'Hãy quét mã QR trước, hoặc dán địa chỉ hiện trong LarmaMic.', '请先扫描二维码，或粘贴 LarmaMic 中显示的地址。',
                 '先に QR コードをスキャンするか、LarmaMic に表示されたアドレスを貼り付けてください。',
                 '먼저 QR 코드를 스캔하거나 LarmaMic에 표시된 주소를 붙여넣으세요.',
                 'Pindai kode QR dulu, atau tempel alamat yang tampil di LarmaMic.',
                 'Primero escanea el código QR o pega la dirección que muestra LarmaMic.',
                 'Scannez d’abord le QR code, ou collez l’adresse affichée par LarmaMic.',
                 'Scanne zuerst den QR-Code oder füge die in LarmaMic angezeigte Adresse ein.',
                 'Сначала отсканируйте QR-код или вставьте адрес, показанный в LarmaMic.'),
}


def parse_ts(path):
    out = {}
    for line in io.open(path, encoding='utf-8'):
        m = re.match(r"\s*'([^']+)':\s*(?:'((?:[^'\\]|\\.)*)'|\"((?:[^\"\\]|\\.)*)\"),\s*$", line)
        if m:
            v = m.group(2) if m.group(2) is not None else m.group(3)
            out[m.group(1)] = v.replace("\\'", "'").replace('\\"', '"')
    return out


def res_name(key):
    return key.replace('.', '_').replace('-', '_')


def to_xml(v):
    has_args = re.search(r'\{\d\}', v) is not None
    v = v.replace('\\', '\\\\')
    v = v.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')
    v = v.replace("'", "\\'").replace('"', '\\"').replace('\n', '\\n')
    if v.startswith('@') or v.startswith('?'):
        v = '\\' + v
    v = re.sub(r'\{(\d)\}', lambda m: '%%%d$s' % (int(m.group(1)) + 1), v) if False else re.sub(r'\{(\d)\}', lambda m: '%' + str(int(m.group(1)) + 1) + '$s', v)
    if has_args:
        v = v.replace('%', '%%').replace('%%' + '1$s', '%1$s')  # restore placeholders below
        v = re.sub(r'%%(\d)\$s', r'%\1$s', v)
    return v, has_args


def main():
    web = {l: parse_ts(os.path.join(WEB, l + '.ts')) for l in LANGS}
    base = set(web['en'])
    for l, d in web.items():
        assert set(d) == base, (l, base ^ set(d))
    order = list(LANGS)
    for i, l in enumerate(order):
        folder = os.path.join(RES, LANGS[l])
        os.makedirs(folder, exist_ok=True)
        lines = ['<?xml version="1.0" encoding="utf-8"?>', '<!-- Generated by android/tools/gen_strings.py. Do not edit by hand. -->', '<resources>']
        if l == 'en':
            lines.append('    <string name="app_name" translatable="false">LarmaMic</string>')
        items = list(web[l].items()) + [(k, vals[i]) for k, vals in EXTRA.items()]
        for k, v in items:
            xml, has_args = to_xml(v)
            attr = '' if has_args or '%' not in xml else ' formatted="false"'
            lines.append('    <string name="%s"%s>%s</string>' % (res_name(k), attr, xml))
        lines.append('</resources>')
        io.open(os.path.join(folder, 'strings.xml'), 'w', encoding='utf-8', newline='\n').write('\n'.join(lines) + '\n')
    print('ok', len(base) + len(EXTRA), 'strings x', len(LANGS))


if __name__ == '__main__':
    main()
