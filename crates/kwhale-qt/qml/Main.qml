import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import QtQuick.Effects
import org.kde.kirigami as Kirigami
import org.dueattendant149.kwhale

Controls.ApplicationWindow {
    id: root
    title: "KWhale"
    width: 1220
    height: 780
    minimumWidth: 960
    minimumHeight: 640
    visible: true
    color: bridge.color_surface

    KwhaleBridge {
        id: bridge
        Component.onCompleted: bridge.init()
    }

    Timer {
        interval: 90
        running: true
        repeat: true
        onTriggered: bridge.poll_updates()
    }

    // Parsed JSON models
    property var homeData: ({ mixes: [], sections: [] })
    property var searchData: ({ history: [], genres: [], topArtists: [], tracks: [], albums: [], artists: [], playlists: [] })
    property var libraryData: ({ tracks: [], playlists: [], albums: [] })
    property var sidebarPlaylists: []
    property var collectionData: ({ kind: "", id: "", title: "", subtitle: "", meta: "", coverUrl: "", canEdit: false, isPublic: false, isFavorite: false, tracks: [], albums: [] })
    property var downloadsData: ({ active: [], downloaded: [] })
    property var historyList: []
    property var profileData: ({ name: "", email: "", enrichments: [] })
    property var queueList: []
    property var lyricsData: ({ synced: [], plain: "" })
    property var accentsCatalog: []
    property var trackInfoData: ({ open: false })
    property var shareData: ({ open: false })

    // Local UI state
    property string searchFilter: "all"
    property string libraryTab: "all"
    property string libraryFilterQuery: ""
    property string artistDiscographyFilter: "all"
    property var selectedActionTrack: null
    property string selectedActionSurface: "home"
    property string uploadSelectedFileUrl: ""

    Connections {
        target: bridge
        function onHome_jsonChanged() {
            try { root.homeData = JSON.parse(bridge.home_json) } catch (e) {}
        }
        function onSearch_jsonChanged() {
            try { root.searchData = JSON.parse(bridge.search_json) } catch (e) {}
        }
        function onLibrary_jsonChanged() {
            try { root.libraryData = JSON.parse(bridge.library_json) } catch (e) {}
        }
        function onSidebar_playlists_jsonChanged() {
            try { root.sidebarPlaylists = JSON.parse(bridge.sidebar_playlists_json) } catch (e) {}
        }
        function onCollection_jsonChanged() {
            try { root.collectionData = JSON.parse(bridge.collection_json) } catch (e) {}
        }
        function onDownloads_jsonChanged() {
            try { root.downloadsData = JSON.parse(bridge.downloads_json) } catch (e) {}
        }
        function onHistory_jsonChanged() {
            try { root.historyList = JSON.parse(bridge.history_json) } catch (e) {}
        }
        function onProfile_jsonChanged() {
            try { root.profileData = JSON.parse(bridge.profile_json) } catch (e) {}
        }
        function onQueue_jsonChanged() {
            try { root.queueList = JSON.parse(bridge.queue_json) } catch (e) {}
        }
        function onLyrics_jsonChanged() {
            try { root.lyricsData = JSON.parse(bridge.lyrics_json) } catch (e) {}
        }
        function onAccents_jsonChanged() {
            try { root.accentsCatalog = JSON.parse(bridge.accents_json) } catch (e) {}
        }
        function onTrack_info_jsonChanged() {
            try {
                var d = JSON.parse(bridge.track_info_json)
                root.trackInfoData = d
                if (d && d.open) trackInfoDialog.open()
            } catch (e) {}
        }
        function onShare_dialog_jsonChanged() {
            try {
                var d = JSON.parse(bridge.share_dialog_json)
                root.shareData = d
                if (d && d.open) shareDialog.open()
            } catch (e) {}
        }
        function onToast_seqChanged() {
            if (bridge.toast_message.length > 0) {
                toastTimer.restart()
            }
        }
    }

    Component.onCompleted: {
        try { root.accentsCatalog = JSON.parse(bridge.accents_json) } catch (e) {}
    }

    // Global Keyboard Shortcuts
    Shortcut {
        sequence: "Space"
        onActivated: bridge.play_pause()
    }
    Shortcut {
        sequence: "Ctrl+Left"
        onActivated: bridge.previous_track()
    }
    Shortcut {
        sequence: "Ctrl+Right"
        onActivated: bridge.next_track()
    }
    Shortcut {
        sequence: "Shift+Left"
        onActivated: bridge.seek_relative_ms(-5000)
    }
    Shortcut {
        sequence: "Shift+Right"
        onActivated: bridge.seek_relative_ms(5000)
    }
    Shortcut {
        sequence: "Ctrl+Up"
        onActivated: bridge.set_volume_level(bridge.volume + 0.05)
    }
    Shortcut {
        sequence: "Ctrl+Down"
        onActivated: bridge.set_volume_level(bridge.volume - 0.05)
    }
    Shortcut {
        sequences: ["Ctrl+F", "Ctrl+K"]
        onActivated: {
            bridge.navigate("search")
            searchInput.forceActiveFocus()
        }
    }
    Shortcut {
        sequence: "Ctrl+L"
        onActivated: bridge.toggle_inspector("lyrics")
    }
    Shortcut {
        sequence: "Ctrl+Q"
        onActivated: bridge.toggle_inspector("queue")
    }
    Shortcut {
        sequence: "M"
        onActivated: bridge.toggle_mute()
    }
    Shortcut {
        sequence: "F11"
        onActivated: bridge.toggle_now_playing()
    }

    // Window-wide Drag-and-Drop Audio File Upload
    DropArea {
        anchors.fill: parent
        onDropped: function(drop) {
            if (drop.hasUrls && drop.urls.length > 0) {
                var u = drop.urls[0].toString()
                var lower = u.toLowerCase()
                if (lower.endsWith(".mp3") || lower.endsWith(".flac") || lower.endsWith(".m4a") ||
                    lower.endsWith(".ogg") || lower.endsWith(".opus") || lower.endsWith(".wav") || lower.endsWith(".aac")) {
                    root.uploadSelectedFileUrl = u
                    uploadTitleField.text = ""
                    uploadArtistField.text = ""
                    uploadAlbumField.text = ""
                    uploadTrackDialog.open()
                }
            }
        }
    }

    // Reusable Track Row Component
    component TrackRowItem : Rectangle {
        property var trackData: ({})
        property string contextKey: ""
        property string surface: "home"
        property bool allowPlaylistEdit: false

        width: ListView.view ? ListView.view.width : parent.width
        height: 58
        radius: 10
        color: trackData.id === bridge.current_track_id
               ? Qt.rgba(0.45, 0.7, 1.0, 0.16)
               : (rowMouse.containsMouse ? bridge.color_surface_container : "transparent")

        MouseArea {
            id: rowMouse
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            onClicked: function(mouse) {
                if (mouse.button === Qt.RightButton) {
                    root.selectedActionTrack = trackData
                    root.selectedActionSurface = surface
                    trackActionsDialog.open()
                } else {
                    if (contextKey.length > 0) {
                        bridge.play_context_track(contextKey, trackData.index, surface)
                    } else {
                        bridge.play_track_now(trackData.id, surface)
                    }
                }
            }
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            spacing: 12

            Controls.Label {
                text: (trackData.index !== undefined ? (trackData.index + 1) : 1).toString()
                color: bridge.color_on_surface_variant
                Layout.preferredWidth: 24
                horizontalAlignment: Text.AlignRight
            }

            Rectangle {
                Layout.preferredWidth: 42
                Layout.preferredHeight: 42
                radius: 8
                color: bridge.color_surface_high
                clip: true
                Image {
                    anchors.fill: parent
                    source: trackData.coverUrl || ""
                    fillMode: Image.PreserveAspectCrop
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2

                RowLayout {
                    spacing: 6
                    Controls.Label {
                        text: trackData.title || ""
                        color: trackData.id === bridge.current_track_id ? bridge.color_primary : bridge.color_on_surface
                        font.bold: true
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    Rectangle {
                        visible: trackData.explicit === true
                        color: bridge.color_surface_high
                        radius: 4
                        implicitWidth: 18
                        implicitHeight: 16
                        Controls.Label {
                            anchors.centerIn: parent
                            text: "E"
                            font.pixelSize: 10
                            font.bold: true
                            color: bridge.color_on_surface_variant
                        }
                    }
                    Controls.Label {
                        visible: trackData.isDownloaded === true
                        text: "⬇"
                        color: bridge.color_primary
                        font.bold: true
                    }
                }

                Controls.Label {
                    text: (trackData.artist || "") + (trackData.album ? (" • " + trackData.album) : "")
                    color: bridge.color_on_surface_variant
                    font.pixelSize: 12
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
            }

            Controls.ToolButton {
                visible: allowPlaylistEdit
                text: "↑"
                onClicked: bridge.move_current_playlist_track(trackData.index, -1)
            }
            Controls.ToolButton {
                visible: allowPlaylistEdit
                text: "↓"
                onClicked: bridge.move_current_playlist_track(trackData.index, 1)
            }

            Controls.ToolButton {
                text: trackData.isLiked ? "♥" : "♡"
                palette.buttonText: trackData.isLiked ? bridge.color_primary : bridge.color_on_surface_variant
                onClicked: bridge.toggle_track_like(trackData.id, surface)
            }

            Controls.Label {
                text: trackData.durationText || "0:00"
                color: bridge.color_on_surface_variant
                Layout.preferredWidth: 44
                horizontalAlignment: Text.AlignRight
            }

            Controls.ToolButton {
                text: "⋯"
                onClicked: {
                    root.selectedActionTrack = trackData
                    root.selectedActionSurface = surface
                    trackActionsDialog.open()
                }
            }
        }
    }

    // Reusable Entity Card Component
    component EntityCardItem : Rectangle {
        property string itemTitle: ""
        property string itemSubtitle: ""
        property string itemBadge: ""
        property string itemCoverUrl: ""
        signal activated()

        width: 162
        height: 214
        radius: 14
        color: cardMouse.containsMouse ? bridge.color_surface_high : bridge.color_surface_container
        border.color: bridge.color_outline_variant
        border.width: 1

        MouseArea {
            id: cardMouse
            anchors.fill: parent
            hoverEnabled: true
            onClicked: parent.activated()
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 10
            spacing: 6

            Rectangle {
                Layout.preferredWidth: 142
                Layout.preferredHeight: 142
                radius: 10
                color: bridge.color_surface_low
                clip: true
                Image {
                    anchors.fill: parent
                    source: itemCoverUrl
                    fillMode: Image.PreserveAspectCrop
                }
            }

            Controls.Label {
                text: itemTitle
                color: bridge.color_on_surface
                font.bold: true
                elide: Text.ElideRight
                Layout.fillWidth: true
            }

            Controls.Label {
                text: (itemBadge.length > 0 ? (itemBadge + " • ") : "") + itemSubtitle
                color: bridge.color_on_surface_variant
                font.pixelSize: 12
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
        }
    }

    // ==================== AUTH VIEW ====================
    Rectangle {
        anchors.fill: parent
        visible: !bridge.authenticated
        color: bridge.color_surface

        property bool registerMode: false

        ColumnLayout {
            anchors.centerIn: parent
            width: Math.min(440, parent.width - 40)
            spacing: 14

            Controls.Label {
                text: "KWhale Music (Qt6 / Kirigami)"
                color: bridge.color_on_surface
                font.pixelSize: 26
                font.bold: true
                Layout.alignment: Qt.AlignHCenter
            }

            Controls.Label {
                text: "Подключитесь к вашему self-hosted серверу KWhale"
                color: bridge.color_on_surface_variant
                Layout.alignment: Qt.AlignHCenter
            }

            Controls.TextField {
                id: authUrlField
                Layout.fillWidth: true
                placeholderText: "Адрес сервера (https://...)"
                text: bridge.server_url
            }

            Controls.CheckBox {
                id: authHttpCheck
                text: "Разрешить HTTP вне локальной сети"
                checked: bridge.allow_insecure_http
            }

            RowLayout {
                Layout.alignment: Qt.AlignHCenter
                spacing: 8
                Controls.Button {
                    text: "Вход"
                    highlighted: !parent.parent.parent.registerMode
                    onClicked: parent.parent.parent.registerMode = false
                }
                Controls.Button {
                    text: "Регистрация по инвайту"
                    highlighted: parent.parent.parent.registerMode
                    onClicked: parent.parent.parent.registerMode = true
                }
            }

            Controls.TextField {
                id: authNameField
                visible: parent.parent.registerMode
                Layout.fillWidth: true
                placeholderText: "Имя пользователя"
            }

            Controls.TextField {
                id: authEmailField
                Layout.fillWidth: true
                placeholderText: "Email"
            }

            Controls.TextField {
                id: authPassField
                Layout.fillWidth: true
                placeholderText: "Пароль (не менее 8 символов)"
                echoMode: TextInput.Password
            }

            Controls.TextField {
                id: authInviteField
                visible: parent.parent.registerMode
                Layout.fillWidth: true
                placeholderText: "Инвайт-токен"
            }

            Controls.Button {
                Layout.fillWidth: true
                highlighted: true
                text: parent.parent.registerMode ? "Зарегистрироваться" : "Войти в KWhale"
                onClicked: {
                    if (parent.parent.registerMode) {
                        bridge.register_account(authUrlField.text, authHttpCheck.checked, authNameField.text, authEmailField.text, authPassField.text, authInviteField.text)
                    } else {
                        bridge.login(authUrlField.text, authHttpCheck.checked, authEmailField.text, authPassField.text)
                    }
                }
            }
        }
    }

    // ==================== MAIN 3-ZONE DESKTOP WORKSPACE ====================
    ColumnLayout {
        anchors.fill: parent
        visible: bridge.authenticated
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // 1. LEFT PERSISTENT SIDEBAR (250px)
            Rectangle {
                Layout.preferredWidth: 250
                Layout.fillHeight: true
                color: bridge.color_surface_low
                border.color: bridge.color_outline_variant
                border.width: 1

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 6

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        Controls.ToolButton {
                            visible: bridge.can_go_back
                            text: "←"
                            onClicked: bridge.navigate_back()
                        }
                        Controls.Label {
                            text: "KWhale"
                            color: bridge.color_on_surface
                            font.pixelSize: 20
                            font.bold: true
                            Layout.fillWidth: true
                        }
                        Controls.ToolButton {
                            text: "+ Трек"
                            Controls.ToolTip.visible: hovered
                            Controls.ToolTip.text: "Загрузить трек на сервер"
                            onClicked: uploadTrackDialog.open()
                        }
                    }

                    Repeater {
                        model: [
                            { id: "home", label: "🏠  Главная" },
                            { id: "search", label: "🔍  Поиск" },
                            { id: "library", label: "📚  Библиотека" },
                            { id: "downloads", label: "⬇  Загрузки" },
                            { id: "history", label: "🕒  История" },
                            { id: "profile", label: "👤  Профиль" },
                            { id: "appearance", label: "🎨  Оформление" },
                            { id: "settings", label: "⚙  Настройки" }
                        ]
                        delegate: Rectangle {
                            Layout.fillWidth: true
                            height: 36
                            radius: 8
                            color: bridge.active_page === modelData.id ? bridge.color_primary_container : (navMouse.containsMouse ? bridge.color_surface_container : "transparent")
                            MouseArea {
                                id: navMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                onClicked: bridge.navigate(modelData.id)
                            }
                            Controls.Label {
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.left: parent.left
                                anchors.leftMargin: 10
                                text: modelData.label
                                color: bridge.active_page === modelData.id ? bridge.color_on_primary_container : bridge.color_on_surface
                                font.bold: bridge.active_page === modelData.id
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.topMargin: 10
                        Controls.Label {
                            text: "МОИ ПЛЕЙЛИСТЫ"
                            color: bridge.color_on_surface_variant
                            font.pixelSize: 11
                            font.bold: true
                            Layout.fillWidth: true
                        }
                        Controls.ToolButton {
                            text: "+"
                            onClicked: {
                                newPlTitle.text = ""
                                newPlDesc.text = ""
                                newPlTrackId.text = ""
                                createPlaylistDialog.open()
                            }
                        }
                    }

                    ListView {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        model: root.sidebarPlaylists
                        spacing: 4
                        delegate: Rectangle {
                            width: ListView.view.width
                            height: 32
                            radius: 6
                            color: plMouse.containsMouse ? bridge.color_surface_container : "transparent"
                            MouseArea {
                                id: plMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                onClicked: bridge.open_playlist(modelData.id)
                            }
                            Controls.Label {
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.margins: 8
                                text: "🎵 " + modelData.title
                                color: bridge.color_on_surface
                                elide: Text.ElideRight
                            }
                        }
                    }
                }
            }

            // 2. CENTER CONTENT AREA
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true

                // ---- HOME PAGE ----
                Controls.ScrollView {
                    anchors.fill: parent
                    visible: bridge.active_page === "home"
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width - 48
                        x: 24
                        y: 20
                        spacing: 20

                        RowLayout {
                            Layout.fillWidth: true
                            Controls.Label {
                                text: bridge.greeting + ", " + bridge.user_name + "!"
                                color: bridge.color_on_surface
                                font.pixelSize: 26
                                font.bold: true
                                Layout.fillWidth: true
                            }
                            Controls.Button {
                                text: "Обновить"
                                onClicked: bridge.reload_home()
                            }
                        }

                        // Hero Wave & AI Prompt-Mix Row
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 16

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 150
                                radius: 16
                                color: bridge.color_primary_container
                                ColumnLayout {
                                    anchors.fill: parent
                                    anchors.margins: 18
                                    Controls.Label {
                                        text: "Моя волна KWhale"
                                        color: bridge.color_on_primary_container
                                        font.pixelSize: 22
                                        font.bold: true
                                    }
                                    Controls.Label {
                                        text: "Персональный поток на основе ваших лайков, истории и времени суток"
                                        color: bridge.color_on_primary_container
                                        wrapMode: Text.WordWrap
                                        Layout.fillWidth: true
                                    }
                                    Controls.Button {
                                        text: "▶ Запустить Мою волну"
                                        highlighted: true
                                        onClicked: bridge.play_personal_wave()
                                    }
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 150
                                radius: 16
                                color: bridge.color_surface_container
                                border.color: bridge.color_outline_variant
                                ColumnLayout {
                                    anchors.fill: parent
                                    anchors.margins: 18
                                    Controls.Label {
                                        text: "AI Промпт-Микс"
                                        color: bridge.color_on_surface
                                        font.pixelSize: 18
                                        font.bold: true
                                    }
                                    Controls.TextField {
                                        id: promptMixInput
                                        Layout.fillWidth: true
                                        placeholderText: "Например: ночной синтвейв для кодинга…"
                                    }
                                    RowLayout {
                                        Controls.Label {
                                            text: "Треков:"
                                            color: bridge.color_on_surface_variant
                                        }
                                        Controls.SpinBox {
                                            id: promptMixSize
                                            from: 10
                                            to: 50
                                            stepSize: 5
                                            value: 25
                                        }
                                        Controls.Button {
                                            text: "Собрать микс"
                                            highlighted: true
                                            onClicked: bridge.play_prompt_mix(promptMixInput.text, promptMixSize.value)
                                        }
                                    }
                                }
                            }
                        }

                        // Personal Mixes
                        ColumnLayout {
                            visible: root.homeData.mixes && root.homeData.mixes.length > 0
                            Layout.fillWidth: true
                            spacing: 10
                            Controls.Label {
                                text: "Персональные миксы"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 12
                                Repeater {
                                    model: root.homeData.mixes || []
                                    delegate: EntityCardItem {
                                        itemTitle: modelData.title
                                        itemSubtitle: modelData.subtitle
                                        itemCoverUrl: modelData.coverUrl
                                        onActivated: bridge.open_mix(modelData.id)
                                    }
                                }
                            }
                        }

                        // Dynamic Home Sections
                        Repeater {
                            model: root.homeData.sections || []
                            delegate: ColumnLayout {
                                property var sec: modelData
                                Layout.fillWidth: true
                                spacing: 8

                                Controls.Label {
                                    text: sec.title || "Рекомендации"
                                    color: bridge.color_on_surface
                                    font.pixelSize: 18
                                    font.bold: true
                                }

                                Repeater {
                                    model: sec.tracks || []
                                    delegate: TrackRowItem {
                                        trackData: modelData
                                        contextKey: sec.contextKey
                                        surface: "home"
                                    }
                                }

                                Flow {
                                    visible: (sec.albums && sec.albums.length > 0) || (sec.artists && sec.artists.length > 0)
                                    Layout.fillWidth: true
                                    spacing: 12
                                    Repeater {
                                        model: sec.albums || []
                                        delegate: EntityCardItem {
                                            itemTitle: modelData.title
                                            itemSubtitle: modelData.subtitle
                                            itemBadge: modelData.badge || ""
                                            itemCoverUrl: modelData.coverUrl
                                            onActivated: bridge.open_album(modelData.id)
                                        }
                                    }
                                    Repeater {
                                        model: sec.artists || []
                                        delegate: EntityCardItem {
                                            itemTitle: modelData.name
                                            itemSubtitle: "Артист"
                                            itemCoverUrl: modelData.coverUrl
                                            onActivated: bridge.open_artist(modelData.id)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // ---- SEARCH PAGE ----
                Controls.ScrollView {
                    anchors.fill: parent
                    visible: bridge.active_page === "search"
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width - 48
                        x: 24
                        y: 20
                        spacing: 16

                        Controls.Label {
                            text: "Поиск"
                            color: bridge.color_on_surface
                            font.pixelSize: 26
                            font.bold: true
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 10
                            Controls.TextField {
                                id: searchInput
                                Layout.fillWidth: true
                                placeholderText: "Треки, альбомы, артисты, плейлисты… (Ctrl+F)"
                                onAccepted: bridge.run_search(text, root.searchFilter)
                            }
                            Controls.Button {
                                text: "Найти"
                                highlighted: true
                                onClicked: bridge.run_search(searchInput.text, root.searchFilter)
                            }
                        }

                        RowLayout {
                            spacing: 8
                            Repeater {
                                model: [
                                    { k: "all", l: "Все" },
                                    { k: "track", l: "Треки" },
                                    { k: "album", l: "Альбомы" },
                                    { k: "artist", l: "Артисты" },
                                    { k: "playlist", l: "Плейлисты" }
                                ]
                                delegate: Controls.Button {
                                    text: modelData.l
                                    highlighted: root.searchFilter === modelData.k
                                    onClicked: {
                                        root.searchFilter = modelData.k
                                        if (searchInput.text.trim().length > 0) {
                                            bridge.run_search(searchInput.text, root.searchFilter)
                                        }
                                    }
                                }
                            }
                        }

                        Controls.Label {
                            visible: bridge.search_partial
                            text: "Догружаем результаты из внешних коннекторов…"
                            color: bridge.color_primary
                        }

                        // Search history chips
                        ColumnLayout {
                            visible: root.searchData.history && root.searchData.history.length > 0
                            Layout.fillWidth: true
                            RowLayout {
                                Controls.Label {
                                    text: "История поиска"
                                    color: bridge.color_on_surface
                                    font.bold: true
                                    Layout.fillWidth: true
                                }
                                Controls.Button {
                                    text: "Очистить"
                                    onClicked: bridge.clear_search_history()
                                }
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 8
                                Repeater {
                                    model: root.searchData.history || []
                                    delegate: Controls.Button {
                                        text: modelData
                                        onClicked: {
                                            searchInput.text = modelData
                                            bridge.run_search(modelData, root.searchFilter)
                                        }
                                    }
                                }
                            }
                        }

                        // Top artists when query is empty
                        ColumnLayout {
                            visible: root.searchData.topArtists && root.searchData.topArtists.length > 0
                            Layout.fillWidth: true
                            Controls.Label {
                                text: "Популярные артисты"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 12
                                Repeater {
                                    model: root.searchData.topArtists || []
                                    delegate: EntityCardItem {
                                        itemTitle: modelData.name
                                        itemSubtitle: "Артист"
                                        itemCoverUrl: modelData.coverUrl
                                        onActivated: bridge.open_artist(modelData.id)
                                    }
                                }
                            }
                        }

                        // Genres when query is empty
                        ColumnLayout {
                            visible: root.searchData.genres && root.searchData.genres.length > 0
                            Layout.fillWidth: true
                            Controls.Label {
                                text: "Жанры и настроения"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 10
                                Repeater {
                                    model: root.searchData.genres || []
                                    delegate: Controls.Button {
                                        text: modelData.name
                                        onClicked: {
                                            searchInput.text = modelData.name
                                            bridge.run_search(modelData.name, root.searchFilter)
                                        }
                                    }
                                }
                            }
                        }

                        // Search Tracks
                        ColumnLayout {
                            visible: root.searchData.tracks && root.searchData.tracks.length > 0
                            Layout.fillWidth: true
                            spacing: 4
                            Controls.Label {
                                text: "Треки"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Repeater {
                                model: root.searchData.tracks || []
                                delegate: TrackRowItem {
                                    trackData: modelData
                                    contextKey: "search_tracks"
                                    surface: "search"
                                }
                            }
                        }

                        // Search Albums
                        ColumnLayout {
                            visible: root.searchData.albums && root.searchData.albums.length > 0
                            Layout.fillWidth: true
                            Controls.Label {
                                text: "Альбомы"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 12
                                Repeater {
                                    model: root.searchData.albums || []
                                    delegate: EntityCardItem {
                                        itemTitle: modelData.title
                                        itemSubtitle: modelData.subtitle
                                        itemBadge: modelData.badge || ""
                                        itemCoverUrl: modelData.coverUrl
                                        onActivated: bridge.open_album(modelData.id)
                                    }
                                }
                            }
                        }

                        // Search Artists
                        ColumnLayout {
                            visible: root.searchData.artists && root.searchData.artists.length > 0
                            Layout.fillWidth: true
                            Controls.Label {
                                text: "Артисты"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 12
                                Repeater {
                                    model: root.searchData.artists || []
                                    delegate: EntityCardItem {
                                        itemTitle: modelData.name
                                        itemSubtitle: "Артист"
                                        itemCoverUrl: modelData.coverUrl
                                        onActivated: bridge.open_artist(modelData.id)
                                    }
                                }
                            }
                        }

                        // Search Playlists
                        ColumnLayout {
                            visible: root.searchData.playlists && root.searchData.playlists.length > 0
                            Layout.fillWidth: true
                            Controls.Label {
                                text: "Плейлисты"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 12
                                Repeater {
                                    model: root.searchData.playlists || []
                                    delegate: EntityCardItem {
                                        itemTitle: modelData.title
                                        itemSubtitle: modelData.subtitle
                                        itemCoverUrl: modelData.coverUrl
                                        onActivated: bridge.open_playlist(modelData.id)
                                    }
                                }
                            }
                        }
                    }
                }

                // ---- LIBRARY PAGE ----
                Controls.ScrollView {
                    anchors.fill: parent
                    visible: bridge.active_page === "library"
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width - 48
                        x: 24
                        y: 20
                        spacing: 16

                        RowLayout {
                            Layout.fillWidth: true
                            Controls.Label {
                                text: "Библиотека"
                                color: bridge.color_on_surface
                                font.pixelSize: 26
                                font.bold: true
                                Layout.fillWidth: true
                            }
                            Controls.Button {
                                text: "+ Плейлист"
                                onClicked: {
                                    newPlTitle.text = ""
                                    newPlDesc.text = ""
                                    newPlTrackId.text = ""
                                    createPlaylistDialog.open()
                                }
                            }
                            Controls.Button {
                                text: "Загрузить трек"
                                onClicked: uploadTrackDialog.open()
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 16
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 110
                                radius: 14
                                color: bridge.color_primary_container
                                ColumnLayout {
                                    anchors.fill: parent
                                    anchors.margins: 16
                                    Controls.Label {
                                        text: "♥ Любимые треки"
                                        color: bridge.color_on_primary_container
                                        font.pixelSize: 20
                                        font.bold: true
                                    }
                                    Controls.Button {
                                        text: "Открыть коллекцию"
                                        highlighted: true
                                        onClicked: bridge.open_liked_tracks()
                                    }
                                }
                            }
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 110
                                radius: 14
                                color: bridge.color_surface_container
                                ColumnLayout {
                                    anchors.fill: parent
                                    anchors.margins: 16
                                    Controls.Label {
                                        text: "⬇ Загруженные офлайн"
                                        color: bridge.color_on_surface
                                        font.pixelSize: 18
                                        font.bold: true
                                    }
                                    Controls.Button {
                                        text: "Открыть загрузки"
                                        onClicked: bridge.navigate("downloads")
                                    }
                                }
                            }
                        }

                        RowLayout {
                            spacing: 8
                            Repeater {
                                model: [
                                    { k: "all", l: "Все" },
                                    { k: "playlists", l: "Плейлисты" },
                                    { k: "albums", l: "Альбомы" },
                                    { k: "tracks", l: "Треки" }
                                ]
                                delegate: Controls.Button {
                                    text: modelData.l
                                    highlighted: root.libraryTab === modelData.k
                                    onClicked: root.libraryTab = modelData.k
                                }
                            }
                        }

                        ColumnLayout {
                            visible: (root.libraryTab === "all" || root.libraryTab === "playlists") && root.libraryData.playlists && root.libraryData.playlists.length > 0
                            Layout.fillWidth: true
                            Controls.Label {
                                text: "Плейлисты"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 12
                                Repeater {
                                    model: root.libraryData.playlists || []
                                    delegate: EntityCardItem {
                                        itemTitle: modelData.title
                                        itemSubtitle: modelData.subtitle
                                        itemCoverUrl: modelData.coverUrl
                                        onActivated: bridge.open_playlist(modelData.id)
                                    }
                                }
                            }
                        }

                        ColumnLayout {
                            visible: (root.libraryTab === "all" || root.libraryTab === "albums") && root.libraryData.albums && root.libraryData.albums.length > 0
                            Layout.fillWidth: true
                            Controls.Label {
                                text: "Альбомы"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 12
                                Repeater {
                                    model: root.libraryData.albums || []
                                    delegate: EntityCardItem {
                                        itemTitle: modelData.title
                                        itemSubtitle: modelData.subtitle
                                        itemBadge: modelData.badge || ""
                                        itemCoverUrl: modelData.coverUrl
                                        onActivated: bridge.open_album(modelData.id)
                                    }
                                }
                            }
                        }

                        ColumnLayout {
                            visible: (root.libraryTab === "all" || root.libraryTab === "tracks") && root.libraryData.tracks && root.libraryData.tracks.length > 0
                            Layout.fillWidth: true
                            spacing: 4
                            Controls.Label {
                                text: "Любимые треки"
                                color: bridge.color_on_surface
                                font.pixelSize: 18
                                font.bold: true
                            }
                            Repeater {
                                model: root.libraryData.tracks || []
                                delegate: TrackRowItem {
                                    trackData: modelData
                                    contextKey: "library_tracks"
                                    surface: "library"
                                }
                            }
                        }
                    }
                }

                // ---- COLLECTION DETAIL PAGE ----
                Controls.ScrollView {
                    anchors.fill: parent
                    visible: bridge.active_page === "collection"
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width - 48
                        x: 24
                        y: 20
                        spacing: 18

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 190
                            radius: 16
                            color: bridge.color_surface_container
                            border.color: bridge.color_outline_variant

                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 18
                                spacing: 20

                                Rectangle {
                                    Layout.preferredWidth: 150
                                    Layout.preferredHeight: 150
                                    radius: 12
                                    color: bridge.color_surface_high
                                    clip: true
                                    Image {
                                        anchors.fill: parent
                                        source: root.collectionData.coverUrl || ""
                                        fillMode: Image.PreserveAspectCrop
                                    }
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 6

                                    Controls.Label {
                                        text: root.collectionData.title || ""
                                        color: bridge.color_on_surface
                                        font.pixelSize: 24
                                        font.bold: true
                                        elide: Text.ElideRight
                                        Layout.fillWidth: true
                                    }
                                    Controls.Label {
                                        text: root.collectionData.subtitle || ""
                                        color: bridge.color_on_surface_variant
                                        wrapMode: Text.WordWrap
                                        Layout.fillWidth: true
                                    }
                                    Controls.Label {
                                        text: root.collectionData.meta || ""
                                        color: bridge.color_on_surface_variant
                                        font.pixelSize: 12
                                    }

                                    Flow {
                                        Layout.fillWidth: true
                                        spacing: 8
                                        Controls.Button {
                                            text: "▶ Слушать"
                                            highlighted: true
                                            onClicked: bridge.play_collection_all(false)
                                        }
                                        Controls.Button {
                                            text: "Перемешать"
                                            onClicked: bridge.play_collection_all(true)
                                        }
                                        Controls.Button {
                                            visible: root.collectionData.kind === "album" || root.collectionData.kind === "artist" || root.collectionData.kind === "playlist"
                                            text: root.collectionData.isFavorite ? "♥ В избранном" : "♡ В избранное"
                                            onClicked: bridge.toggle_collection_favorite()
                                        }
                                        Controls.Button {
                                            visible: root.collectionData.kind === "album" || root.collectionData.kind === "playlist"
                                            text: "Лайкнуть все"
                                            onClicked: bridge.like_all_collection_tracks()
                                        }
                                        Controls.Button {
                                            visible: root.collectionData.kind === "artist"
                                            text: "Синхронизировать дискографию"
                                            onClicked: bridge.sync_current_artist()
                                        }
                                        Controls.Button {
                                            visible: root.collectionData.kind === "artist"
                                            text: "Все треки артиста"
                                            onClicked: bridge.open_artist_all_tracks(root.collectionData.id, root.collectionData.title)
                                        }
                                        Controls.Button {
                                            visible: root.collectionData.kind === "album" || root.collectionData.kind === "artist"
                                            text: "Поделиться…"
                                            onClicked: bridge.request_share(root.collectionData.kind, root.collectionData.id, root.collectionData.title)
                                        }
                                        Controls.Button {
                                            visible: root.collectionData.kind === "playlist" && root.collectionData.canEdit
                                            text: "Изменить"
                                            onClicked: {
                                                editPlTitle.text = root.collectionData.title || ""
                                                editPlDesc.text = root.collectionData.subtitle || ""
                                                editPlPublic.checked = root.collectionData.isPublic === true
                                                editPlaylistDialog.open()
                                            }
                                        }
                                        Controls.Button {
                                            visible: root.collectionData.kind === "playlist" && root.collectionData.canEdit
                                            text: "Обложка…"
                                            onClicked: playlistCoverFileDialog.open()
                                        }
                                        Controls.Button {
                                            visible: root.collectionData.kind === "playlist" && root.collectionData.canEdit
                                            text: "Удалить плейлист"
                                            onClicked: bridge.delete_current_playlist()
                                        }
                                    }
                                }
                            }
                        }

                        // Tracks list in collection
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 4
                            Repeater {
                                model: root.collectionData.tracks || []
                                delegate: TrackRowItem {
                                    trackData: modelData
                                    contextKey: "collection_tracks"
                                    surface: root.collectionData.kind || "playlist"
                                    allowPlaylistEdit: root.collectionData.kind === "playlist" && root.collectionData.canEdit === true
                                }
                            }
                        }

                        // Artist Discography section with Singles/EP filter
                        ColumnLayout {
                            visible: root.collectionData.albums && root.collectionData.albums.length > 0
                            Layout.fillWidth: true
                            spacing: 10

                            RowLayout {
                                Layout.fillWidth: true
                                Controls.Label {
                                    text: "Дискография"
                                    color: bridge.color_on_surface
                                    font.pixelSize: 18
                                    font.bold: true
                                    Layout.fillWidth: true
                                }
                                Repeater {
                                    model: [
                                        { k: "all", l: "Все" },
                                        { k: "albums", l: "Альбомы" },
                                        { k: "singles", l: "Синглы и EP" }
                                    ]
                                    delegate: Controls.Button {
                                        text: modelData.l
                                        highlighted: root.artistDiscographyFilter === modelData.k
                                        onClicked: root.artistDiscographyFilter = modelData.k
                                    }
                                }
                            }

                            Flow {
                                Layout.fillWidth: true
                                spacing: 12
                                Repeater {
                                    model: root.collectionData.albums || []
                                    delegate: EntityCardItem {
                                        visible: root.artistDiscographyFilter === "all" ||
                                                 (root.artistDiscographyFilter === "singles" && modelData.isSingleLike) ||
                                                 (root.artistDiscographyFilter === "albums" && !modelData.isSingleLike)
                                        itemTitle: modelData.title
                                        itemSubtitle: modelData.subtitle
                                        itemBadge: modelData.badge || ""
                                        itemCoverUrl: modelData.coverUrl
                                        onActivated: bridge.open_album(modelData.id)
                                    }
                                }
                            }
                        }
                    }
                }

                // ---- DOWNLOADS PAGE ----
                Controls.ScrollView {
                    anchors.fill: parent
                    visible: bridge.active_page === "downloads"
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width - 48
                        x: 24
                        y: 20
                        spacing: 14

                        Controls.Label {
                            text: "Загрузки (офлайн)"
                            color: bridge.color_on_surface
                            font.pixelSize: 26
                            font.bold: true
                        }

                        Repeater {
                            model: root.downloadsData.active || []
                            delegate: Rectangle {
                                Layout.fillWidth: true
                                height: 48
                                radius: 8
                                color: bridge.color_surface_container
                                RowLayout {
                                    anchors.fill: parent
                                    anchors.margins: 10
                                    Controls.Label {
                                        text: modelData.title + " — " + modelData.artist + " (" + modelData.status + ")"
                                        color: bridge.color_on_surface
                                        Layout.fillWidth: true
                                    }
                                    Controls.Button {
                                        text: "Отмена"
                                        onClicked: bridge.cancel_active_download(modelData.id)
                                    }
                                }
                            }
                        }

                        Repeater {
                            model: root.downloadsData.downloaded || []
                            delegate: TrackRowItem {
                                trackData: modelData
                                contextKey: "downloads_tracks"
                                surface: "library"
                            }
                        }
                    }
                }

                // ---- HISTORY PAGE ----
                Controls.ScrollView {
                    anchors.fill: parent
                    visible: bridge.active_page === "history"
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width - 48
                        x: 24
                        y: 20
                        spacing: 14

                        RowLayout {
                            Layout.fillWidth: true
                            Controls.Label {
                                text: "История прослушивания"
                                color: bridge.color_on_surface
                                font.pixelSize: 26
                                font.bold: true
                                Layout.fillWidth: true
                            }
                            Controls.Button {
                                text: "Очистить историю"
                                onClicked: bridge.clear_play_history()
                            }
                        }

                        Repeater {
                            model: root.historyList || []
                            delegate: TrackRowItem {
                                trackData: modelData
                                contextKey: "history_tracks"
                                surface: "library"
                            }
                        }
                    }
                }

                // ---- PROFILE & ENRICHMENTS PAGE ----
                Controls.ScrollView {
                    anchors.fill: parent
                    visible: bridge.active_page === "profile"
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width - 48
                        x: 24
                        y: 20
                        spacing: 16

                        Controls.Label {
                            text: "Профиль и обогащение метаданных"
                            color: bridge.color_on_surface
                            font.pixelSize: 26
                            font.bold: true
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 10
                            Controls.TextField {
                                id: profileNameInput
                                Layout.fillWidth: true
                                text: bridge.user_name
                                placeholderText: "Отображаемое имя"
                            }
                            Controls.Button {
                                text: "Сохранить имя"
                                onClicked: bridge.update_profile_name(profileNameInput.text)
                            }
                            Controls.Button {
                                text: "Выйти из аккаунта"
                                onClicked: bridge.logout()
                            }
                        }

                        Controls.Label {
                            text: "Предложения обогащения метаданных (автопоиск альбомов, годов и жанров)"
                            color: bridge.color_on_surface
                            font.pixelSize: 18
                            font.bold: true
                        }

                        Repeater {
                            model: root.profileData.enrichments || []
                            delegate: Rectangle {
                                Layout.fillWidth: true
                                height: 64
                                radius: 10
                                color: bridge.color_surface_container
                                RowLayout {
                                    anchors.fill: parent
                                    anchors.margins: 12
                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        Controls.Label {
                                            text: modelData.title
                                            color: bridge.color_on_surface
                                            font.bold: true
                                        }
                                        Controls.Label {
                                            text: modelData.summary
                                            color: bridge.color_on_surface_variant
                                            font.pixelSize: 12
                                        }
                                    }
                                    Controls.Button {
                                        text: "Подтвердить"
                                        highlighted: true
                                        onClicked: bridge.confirm_enrichment(modelData.id)
                                    }
                                    Controls.Button {
                                        text: "Отклонить"
                                        onClicked: bridge.reject_enrichment(modelData.id)
                                    }
                                }
                            }
                        }
                    }
                }

                // ---- APPEARANCE PAGE (129 ACCENTS + 4 MODES) ----
                Controls.ScrollView {
                    anchors.fill: parent
                    visible: bridge.active_page === "appearance"
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width - 48
                        x: 24
                        y: 20
                        spacing: 16

                        Controls.Label {
                            text: "Оформление и акценты (129 палитр)"
                            color: bridge.color_on_surface
                            font.pixelSize: 26
                            font.bold: true
                        }

                        RowLayout {
                            spacing: 10
                            Repeater {
                                model: [
                                    { m: "system", l: "Системная" },
                                    { m: "dark", l: "Тёмная" },
                                    { m: "light", l: "Светлая" },
                                    { m: "amoled", l: "AMOLED (чистый чёрный)" }
                                ]
                                delegate: Controls.Button {
                                    text: modelData.l
                                    highlighted: bridge.theme_mode === modelData.m
                                    onClicked: bridge.set_theme(modelData.m, bridge.accent_id, bridge.follow_system_accent)
                                }
                            }
                        }

                        Controls.CheckBox {
                            text: "Следовать системному акценту рабочего стола"
                            checked: bridge.follow_system_accent
                            onToggled: bridge.set_theme(bridge.theme_mode, bridge.accent_id, checked)
                        }

                        Repeater {
                            model: root.accentsCatalog || []
                            delegate: ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 8
                                Controls.Label {
                                    text: modelData.title
                                    color: bridge.color_on_surface
                                    font.pixelSize: 18
                                    font.bold: true
                                }
                                Flow {
                                    Layout.fillWidth: true
                                    spacing: 8
                                    Repeater {
                                        model: modelData.items || []
                                        delegate: Rectangle {
                                            width: 145
                                            height: 36
                                            radius: 18
                                            color: bridge.accent_id === modelData.id ? bridge.color_primary_container : bridge.color_surface_container
                                            border.color: bridge.accent_id === modelData.id ? bridge.color_primary : bridge.color_outline_variant
                                            border.width: bridge.accent_id === modelData.id ? 2 : 1
                                            MouseArea {
                                                anchors.fill: parent
                                                onClicked: bridge.set_theme(bridge.theme_mode, modelData.id, false)
                                            }
                                            RowLayout {
                                                anchors.fill: parent
                                                anchors.leftMargin: 10
                                                anchors.rightMargin: 10
                                                spacing: 8
                                                Rectangle {
                                                    width: 16
                                                    height: 16
                                                    radius: 8
                                                    color: modelData.hex
                                                }
                                                Controls.Label {
                                                    text: modelData.name
                                                    color: bridge.color_on_surface
                                                    elide: Text.ElideRight
                                                    Layout.fillWidth: true
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // ---- SETTINGS PAGE ----
                Controls.ScrollView {
                    anchors.fill: parent
                    visible: bridge.active_page === "settings"
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width - 48
                        x: 24
                        y: 20
                        spacing: 16

                        Controls.Label {
                            text: "Настройки"
                            color: bridge.color_on_surface
                            font.pixelSize: 26
                            font.bold: true
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 160
                            radius: 14
                            color: bridge.color_surface_container
                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 16
                                Controls.Label {
                                    text: "Сервер KWhale (/v1)"
                                    color: bridge.color_on_surface
                                    font.bold: true
                                }
                                Controls.TextField {
                                    id: settingsUrlField
                                    Layout.fillWidth: true
                                    text: bridge.server_url
                                }
                                Controls.CheckBox {
                                    id: settingsHttpCheck
                                    text: "Разрешить HTTP вне локальной сети"
                                    checked: bridge.allow_insecure_http
                                }
                                RowLayout {
                                    Controls.Button {
                                        text: "Сохранить сервер"
                                        highlighted: true
                                        onClicked: bridge.save_server(settingsUrlField.text, settingsHttpCheck.checked)
                                    }
                                    Controls.Button {
                                        text: "Проверить соединение"
                                        onClicked: bridge.check_server_health()
                                    }
                                    Controls.Label {
                                        text: bridge.server_status
                                        color: bridge.color_on_surface_variant
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 100
                            radius: 14
                            color: bridge.color_surface_container
                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 16
                                Controls.Label {
                                    text: "Нативный движок интерфейса (при следующем запуске)"
                                    color: bridge.color_on_surface
                                    font.bold: true
                                }
                                RowLayout {
                                    Repeater {
                                        model: [
                                            { k: "auto", l: "Авто (по рабочему столу)" },
                                            { k: "adw", l: "GNOME (GTK4 + libadwaita)" },
                                            { k: "qt", l: "KDE (Qt6 Quick + Kirigami)" }
                                        ]
                                        delegate: Controls.Button {
                                            text: modelData.l
                                            highlighted: bridge.ui_backend === modelData.k
                                            onClicked: bridge.set_ui_backend_pref(modelData.k)
                                        }
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 190
                            radius: 14
                            color: bridge.color_surface_container
                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 16
                                Controls.Label {
                                    text: "AI Промпт-Микс (BYOK OpenAI-совместимый LLM)"
                                    color: bridge.color_on_surface
                                    font.bold: true
                                }
                                Controls.TextField {
                                    id: llmEpField
                                    Layout.fillWidth: true
                                    placeholderText: "LLM Endpoint URL"
                                    text: bridge.llm_endpoint
                                }
                                Controls.TextField {
                                    id: llmKeyField
                                    Layout.fillWidth: true
                                    placeholderText: "LLM API Key"
                                    echoMode: TextInput.Password
                                    text: bridge.llm_api_key
                                }
                                Controls.TextField {
                                    id: llmModelField
                                    Layout.fillWidth: true
                                    placeholderText: "LLM Модель"
                                    text: bridge.llm_model
                                }
                                Controls.Button {
                                    text: "Сохранить настройки AI"
                                    onClicked: bridge.save_llm_settings(llmEpField.text, llmKeyField.text, llmModelField.text)
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 110
                            radius: 14
                            color: bridge.color_surface_container
                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 16
                                Controls.CheckBox {
                                    text: "Предзагрузка следующих 5 треков очереди"
                                    checked: bridge.prefetch_enabled
                                    onToggled: bridge.set_prefetch(checked)
                                }
                                RowLayout {
                                    Controls.Label {
                                        text: "Кэш аудиопотоков (лимит 1 ГБ) • Занято: " + bridge.cache_used_label
                                        color: bridge.color_on_surface
                                        Layout.fillWidth: true
                                    }
                                    Controls.Button {
                                        text: "Очистить кэш"
                                        onClicked: bridge.clear_playback_cache()
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 3. RIGHT COLLAPSIBLE INSPECTOR (QUEUE + KARAOKE LYRICS, 340px)
            Rectangle {
                visible: bridge.inspector_open
                Layout.preferredWidth: 340
                Layout.fillHeight: true
                color: bridge.color_surface_low
                border.color: bridge.color_outline_variant
                border.width: 1

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 10

                    RowLayout {
                        Layout.fillWidth: true
                        Controls.Button {
                            text: "Очередь"
                            Layout.fillWidth: true
                            highlighted: bridge.inspector_tab === "queue"
                            onClicked: bridge.toggle_inspector("queue")
                        }
                        Controls.Button {
                            text: "Текст песни"
                            Layout.fillWidth: true
                            highlighted: bridge.inspector_tab === "lyrics"
                            onClicked: bridge.toggle_inspector("lyrics")
                        }
                        Controls.ToolButton {
                            text: "✕"
                            onClicked: bridge.toggle_inspector(bridge.inspector_tab)
                        }
                    }

                    // Queue ListView
                    ListView {
                        visible: bridge.inspector_tab === "queue"
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        model: root.queueList
                        spacing: 4
                        delegate: Rectangle {
                            width: ListView.view.width
                            height: 50
                            radius: 8
                            color: modelData.isCurrent ? bridge.color_primary_container : "transparent"
                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 8
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2
                                    Controls.Label {
                                        text: modelData.title
                                        color: modelData.isCurrent ? bridge.color_on_primary_container : bridge.color_on_surface
                                        font.bold: true
                                        elide: Text.ElideRight
                                        Layout.fillWidth: true
                                    }
                                    Controls.Label {
                                        text: modelData.artist
                                        color: bridge.color_on_surface_variant
                                        font.pixelSize: 11
                                        elide: Text.ElideRight
                                        Layout.fillWidth: true
                                    }
                                }
                                Controls.ToolButton {
                                    text: "▶"
                                    onClicked: bridge.jump_to_queue(modelData.index)
                                }
                                Controls.ToolButton {
                                    text: "↑"
                                    enabled: modelData.index > 0
                                    onClicked: bridge.move_queue_item(modelData.index, modelData.index - 1)
                                }
                                Controls.ToolButton {
                                    text: "✕"
                                    onClicked: bridge.remove_queue_item(modelData.index)
                                }
                            }
                        }
                    }

                    // Synchronized Karaoke Lyrics ListView
                    ListView {
                        id: inspectorLyricsList
                        visible: bridge.inspector_tab === "lyrics" && root.lyricsData.synced && root.lyricsData.synced.length > 0
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        model: root.lyricsData.synced || []
                        spacing: 6
                        onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Center)
                        Connections {
                            target: bridge
                            function onActive_lyric_indexChanged() {
                                if (bridge.active_lyric_index >= 0) {
                                    inspectorLyricsList.currentIndex = bridge.active_lyric_index
                                }
                            }
                        }
                        delegate: Rectangle {
                            width: ListView.view.width
                            implicitHeight: lyrText.implicitHeight + 14
                            radius: 8
                            color: modelData.index === bridge.active_lyric_index ? bridge.color_primary_container : "transparent"
                            MouseArea {
                                anchors.fill: parent
                                onClicked: bridge.seek_ms(modelData.startMs)
                            }
                            Controls.Label {
                                id: lyrText
                                anchors.fill: parent
                                anchors.margins: 7
                                text: modelData.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: modelData.index === bridge.active_lyric_index ? 18 : 14
                                font.bold: modelData.index === bridge.active_lyric_index
                                color: modelData.index === bridge.active_lyric_index ? bridge.color_primary : bridge.color_on_surface_variant
                            }
                        }
                    }

                    Controls.ScrollView {
                        visible: bridge.inspector_tab === "lyrics" && (!root.lyricsData.synced || root.lyricsData.synced.length === 0)
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        Controls.Label {
                            width: 310
                            text: root.lyricsData.plain || "Текст песни недоступен"
                            color: bridge.color_on_surface_variant
                            wrapMode: Text.WordWrap
                        }
                    }
                }
            }
        }

        // 4. BOTTOM PERSISTENT DESKTOP PLAYER DOCK BAR (84px)
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 84
            color: bridge.is_amoled ? "#050505" : bridge.color_surface_low
            border.color: bridge.color_outline_variant
            border.width: 1

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 16
                anchors.rightMargin: 16
                spacing: 16

                // Left: Cover + Title + Artist + Like + Flag + Menu
                RowLayout {
                    Layout.preferredWidth: 320
                    spacing: 10

                    Rectangle {
                        Layout.preferredWidth: 56
                        Layout.preferredHeight: 56
                        radius: 10
                        color: bridge.color_surface_high
                        clip: true
                        Image {
                            anchors.fill: parent
                            source: bridge.current_cover_url
                            fillMode: Image.PreserveAspectCrop
                        }
                        MouseArea {
                            anchors.fill: parent
                            onClicked: bridge.toggle_now_playing()
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Controls.Label {
                            text: bridge.current_title
                            color: bridge.color_on_surface
                            font.bold: true
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        Controls.Label {
                            text: bridge.current_artist
                            color: bridge.color_on_surface_variant
                            font.pixelSize: 12
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                            MouseArea {
                                anchors.fill: parent
                                onClicked: {
                                    if (bridge.current_artist_id.length > 0) {
                                        bridge.open_artist(bridge.current_artist_id)
                                    }
                                }
                            }
                        }
                    }

                    Controls.ToolButton {
                        text: bridge.current_liked ? "♥" : "♡"
                        palette.buttonText: bridge.current_liked ? bridge.color_primary : bridge.color_on_surface
                        onClicked: {
                            if (bridge.current_track_id.length > 0) {
                                bridge.toggle_track_like(bridge.current_track_id, "player")
                            }
                        }
                    }

                    Controls.ToolButton {
                        text: "⚑"
                        Controls.ToolTip.visible: hovered
                        Controls.ToolTip.text: "Пожаловаться на цензуру в 1 клик"
                        onClicked: {
                            if (bridge.current_track_id.length > 0) {
                                bridge.report_track_censorship(bridge.current_track_id)
                            }
                        }
                    }
                }

                // Center: Transport + Seek Slider
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 4

                    RowLayout {
                        Layout.alignment: Qt.AlignHCenter
                        spacing: 12

                        Controls.ToolButton {
                            text: "🔀"
                            highlighted: bridge.shuffle
                            onClicked: bridge.toggle_shuffle()
                        }
                        Controls.ToolButton {
                            text: "⏮"
                            onClicked: bridge.previous_track()
                        }
                        Controls.Button {
                            text: bridge.is_playing ? "⏸" : "▶"
                            highlighted: true
                            onClicked: bridge.play_pause()
                        }
                        Controls.ToolButton {
                            text: "⏭"
                            onClicked: bridge.next_track()
                        }
                        Controls.ToolButton {
                            text: bridge.repeat_mode === "one" ? "🔂" : "🔁"
                            highlighted: bridge.repeat_mode !== "off"
                            onClicked: bridge.cycle_repeat()
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        Controls.Label {
                            text: bridge.position_text
                            color: bridge.color_on_surface_variant
                            font.pixelSize: 11
                        }
                        Controls.Slider {
                            id: seekSlider
                            Layout.fillWidth: true
                            from: 0
                            to: 1000
                            value: pressed ? value : bridge.progress_permille
                            onMoved: bridge.seek_permille(Math.round(value))
                        }
                        Controls.Label {
                            text: bridge.duration_text
                            color: bridge.color_on_surface_variant
                            font.pixelSize: 11
                        }
                    }
                }

                // Right: Info + Lyrics + Queue + Mute + Volume + Fullscreen
                RowLayout {
                    spacing: 6
                    Controls.ToolButton {
                        text: "ⓘ"
                        Controls.ToolTip.visible: hovered
                        Controls.ToolTip.text: "Сведения о треке и качестве"
                        onClicked: {
                            if (bridge.current_track_id.length > 0) {
                                bridge.request_track_info(bridge.current_track_id)
                            }
                        }
                    }
                    Controls.ToolButton {
                        text: "📝"
                        Controls.ToolTip.visible: hovered
                        Controls.ToolTip.text: "Текст песни (Ctrl+L)"
                        highlighted: bridge.inspector_open && bridge.inspector_tab === "lyrics"
                        onClicked: bridge.toggle_inspector("lyrics")
                    }
                    Controls.ToolButton {
                        text: "☰"
                        Controls.ToolTip.visible: hovered
                        Controls.ToolTip.text: "Очередь (Ctrl+Q)"
                        highlighted: bridge.inspector_open && bridge.inspector_tab === "queue"
                        onClicked: bridge.toggle_inspector("queue")
                    }
                    Controls.ToolButton {
                        text: bridge.muted ? "🔇" : "🔊"
                        onClicked: bridge.toggle_mute()
                    }
                    Controls.Slider {
                        Layout.preferredWidth: 96
                        from: 0.0
                        to: 1.0
                        value: bridge.muted ? 0.0 : bridge.volume
                        onMoved: bridge.set_volume_level(value)
                    }
                    Controls.ToolButton {
                        text: "⛶"
                        Controls.ToolTip.visible: hovered
                        Controls.ToolTip.text: "Полноэкранный плеер (F11)"
                        onClicked: bridge.toggle_now_playing()
                    }
                }
            }
        }
    }

    // ==================== FULLSCREEN NOW PLAYING OVERLAY WITH BLURRED COVER ====================
    Rectangle {
        anchors.fill: parent
        visible: bridge.now_playing_open
        color: bridge.color_surface

        Image {
            id: bgBlurSource
            anchors.fill: parent
            source: bridge.current_cover_url
            fillMode: Image.PreserveAspectCrop
            visible: false
        }

        MultiEffect {
            anchors.fill: bgBlurSource
            source: bgBlurSource
            blurEnabled: true
            blur: 1.0
            blurMax: 64
            opacity: 0.35
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 32
            spacing: 20

            RowLayout {
                Layout.fillWidth: true
                Controls.Button {
                    text: "↓ Свернуть плеер (F11)"
                    onClicked: bridge.toggle_now_playing()
                }
                Item { Layout.fillWidth: true }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 40

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignVCenter
                    spacing: 14

                    Rectangle {
                        Layout.preferredWidth: 320
                        Layout.preferredHeight: 320
                        Layout.alignment: Qt.AlignHCenter
                        radius: 20
                        color: bridge.color_surface_high
                        clip: true
                        Image {
                            anchors.fill: parent
                            source: bridge.current_cover_url
                            fillMode: Image.PreserveAspectCrop
                        }
                    }

                    Controls.Label {
                        text: bridge.current_title
                        color: bridge.color_on_surface
                        font.pixelSize: 28
                        font.bold: true
                        Layout.alignment: Qt.AlignHCenter
                    }
                    Controls.Label {
                        text: bridge.current_artist
                        color: bridge.color_on_surface_variant
                        font.pixelSize: 18
                        Layout.alignment: Qt.AlignHCenter
                    }
                }

                ListView {
                    id: npLyricsList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    model: root.lyricsData.synced || []
                    spacing: 10
                    delegate: Rectangle {
                        width: ListView.view.width
                        implicitHeight: npLyrLbl.implicitHeight + 16
                        radius: 10
                        color: modelData.index === bridge.active_lyric_index ? bridge.color_primary_container : "transparent"
                        MouseArea {
                            anchors.fill: parent
                            onClicked: bridge.seek_ms(modelData.startMs)
                        }
                        Controls.Label {
                            id: npLyrLbl
                            anchors.fill: parent
                            anchors.margins: 8
                            text: modelData.text
                            wrapMode: Text.WordWrap
                            font.pixelSize: modelData.index === bridge.active_lyric_index ? 24 : 17
                            font.bold: modelData.index === bridge.active_lyric_index
                            color: modelData.index === bridge.active_lyric_index ? bridge.color_primary : bridge.color_on_surface_variant
                        }
                    }
                }
            }
        }
    }

    // ==================== DIALOGS & PORTALS ====================

    // 1. Track Context Menu / Actions Dialog
    Controls.Dialog {
        id: trackActionsDialog
        anchors.centerIn: parent
        width: 420
        modal: true
        title: root.selectedActionTrack ? root.selectedActionTrack.title : "Действия с треком"
        standardButtons: Controls.Dialog.Close

        ColumnLayout {
            width: parent.width
            spacing: 6

            Controls.Button {
                Layout.fillWidth: true
                text: "▶ Воспроизвести сейчас"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.play_track_now(root.selectedActionTrack.id, root.selectedActionSurface)
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: "⏭ Воспроизвести следующим"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.play_track_next(root.selectedActionTrack.id, root.selectedActionSurface)
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: "➕ Добавить в очередь"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.add_track_to_queue(root.selectedActionTrack.id, root.selectedActionSurface)
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: (root.selectedActionTrack && root.selectedActionTrack.isLiked) ? "♥ Убрать из Любимых" : "♡ Добавить в Любимые"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.toggle_track_like(root.selectedActionTrack.id, root.selectedActionSurface)
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: (root.selectedActionTrack && root.selectedActionTrack.isDisliked) ? "Снять отметку «Не нравится»" : "👎 Не нравится"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.toggle_track_dislike(root.selectedActionTrack.id, root.selectedActionSurface)
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: "📁 Добавить в плейлист…"
                onClicked: {
                    trackActionsDialog.close()
                    addToPlaylistDialog.open()
                }
            }
            Controls.Button {
                visible: bridge.active_page === "collection" && root.collectionData.kind === "playlist" && root.collectionData.canEdit === true
                Layout.fillWidth: true
                text: "🗑 Удалить из этого плейлиста"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.remove_track_from_current_playlist(root.selectedActionTrack.id)
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: "📻 Волна по треку (похожие)"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.start_track_radio(root.selectedActionTrack.id)
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: (root.selectedActionTrack && root.selectedActionTrack.isDownloaded) ? "Удалить офлайн-копию" : "⬇ Скачать для офлайн-прослушивания"
                onClicked: {
                    trackActionsDialog.close()
                    if (root.selectedActionTrack.isDownloaded) {
                        bridge.remove_downloaded_track(root.selectedActionTrack.id)
                    } else {
                        bridge.download_track(root.selectedActionTrack.id)
                    }
                }
            }
            Controls.Button {
                visible: root.selectedActionTrack && root.selectedActionTrack.artistId && root.selectedActionTrack.artistId.length > 0
                Layout.fillWidth: true
                text: "👤 Перейти к артисту"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.open_artist(root.selectedActionTrack.artistId)
                }
            }
            Controls.Button {
                visible: root.selectedActionTrack && root.selectedActionTrack.albumId && root.selectedActionTrack.albumId.length > 0
                Layout.fillWidth: true
                text: "💿 Перейти к альбому"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.open_album(root.selectedActionTrack.albumId)
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: "🔗 Поделиться треком…"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.request_share("track", root.selectedActionTrack.id, root.selectedActionTrack.title)
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: (root.selectedActionTrack && root.selectedActionTrack.isCensorshipReported) ? "Отменить жалобу на цензуру" : "⚑ Пожаловаться на цензуру (в 1 клик)"
                onClicked: {
                    trackActionsDialog.close()
                    if (root.selectedActionTrack.isCensorshipReported) {
                        bridge.cancel_track_censorship(root.selectedActionTrack.id)
                    } else {
                        bridge.report_track_censorship(root.selectedActionTrack.id)
                    }
                }
            }
            Controls.Button {
                Layout.fillWidth: true
                text: "ⓘ Сведения о треке и качестве"
                onClicked: {
                    trackActionsDialog.close()
                    bridge.request_track_info(root.selectedActionTrack.id)
                }
            }
        }
    }

    // 2. Add to Playlist Dialog
    Controls.Dialog {
        id: addToPlaylistDialog
        anchors.centerIn: parent
        width: 400
        modal: true
        title: "Добавить в плейлист"
        standardButtons: Controls.Dialog.Close

        ColumnLayout {
            width: parent.width
            spacing: 8

            Controls.Button {
                Layout.fillWidth: true
                highlighted: true
                text: "+ Создать новый плейлист"
                onClicked: {
                    addToPlaylistDialog.close()
                    newPlTitle.text = ""
                    newPlDesc.text = ""
                    newPlTrackId.text = root.selectedActionTrack ? root.selectedActionTrack.id : ""
                    createPlaylistDialog.open()
                }
            }

            Repeater {
                model: root.sidebarPlaylists || []
                delegate: Controls.Button {
                    Layout.fillWidth: true
                    text: modelData.title + " (" + modelData.subtitle + ")"
                    onClicked: {
                        addToPlaylistDialog.close()
                        if (root.selectedActionTrack) {
                            bridge.add_track_to_playlist(modelData.id, root.selectedActionTrack.id)
                        }
                    }
                }
            }
        }
    }

    // 3. Create Playlist Dialog
    Controls.Dialog {
        id: createPlaylistDialog
        anchors.centerIn: parent
        width: 400
        modal: true
        title: "Новый плейлист"
        standardButtons: Controls.Dialog.Ok | Controls.Dialog.Cancel
        onAccepted: bridge.create_playlist(newPlTitle.text, newPlDesc.text, newPlPublic.checked, newPlTrackId.text)

        ColumnLayout {
            width: parent.width
            spacing: 10
            Controls.TextField { id: newPlTitle; Layout.fillWidth: true; placeholderText: "Название плейлиста" }
            Controls.TextField { id: newPlDesc; Layout.fillWidth: true; placeholderText: "Описание (необязательно)" }
            Controls.CheckBox { id: newPlPublic; text: "Публичный плейлист" }
            Controls.TextField { id: newPlTrackId; visible: false }
        }
    }

    // 4. Edit Playlist Dialog
    Controls.Dialog {
        id: editPlaylistDialog
        anchors.centerIn: parent
        width: 400
        modal: true
        title: "Редактировать плейлист"
        standardButtons: Controls.Dialog.Ok | Controls.Dialog.Cancel
        onAccepted: bridge.update_current_playlist(editPlTitle.text, editPlDesc.text, editPlPublic.checked)

        ColumnLayout {
            width: parent.width
            spacing: 10
            Controls.TextField { id: editPlTitle; Layout.fillWidth: true; placeholderText: "Название плейлиста" }
            Controls.TextField { id: editPlDesc; Layout.fillWidth: true; placeholderText: "Описание" }
            Controls.CheckBox { id: editPlPublic; text: "Публичный плейлист" }
        }
    }

    // 5. Track Info Dialog
    Controls.Dialog {
        id: trackInfoDialog
        anchors.centerIn: parent
        width: 440
        modal: true
        title: "Сведения о треке и качестве"
        standardButtons: Controls.Dialog.Close

        ColumnLayout {
            width: parent.width
            spacing: 6
            Controls.Label { text: "Трек: " + (root.trackInfoData.title || ""); font.bold: true }
            Controls.Label { text: "Артист: " + (root.trackInfoData.artist || "") }
            Controls.Label { text: "ID: " + (root.trackInfoData.id || "") }
            Controls.Label { text: "ISRC: " + (root.trackInfoData.isrc || "—") }
            Controls.Label { text: "Кодек: " + (root.trackInfoData.codec || "—") }
            Controls.Label { text: "Битрейт: " + (root.trackInfoData.bitrate || "—") }
            Controls.Label { text: "Частота: " + (root.trackInfoData.sampleRate || "—") }
            Controls.Label { text: "Размер: " + (root.trackInfoData.size || "—") }
        }
    }

    // 6. Share Dialog
    Controls.Dialog {
        id: shareDialog
        anchors.centerIn: parent
        width: 460
        modal: true
        title: "Поделиться: " + (root.shareData.title || "")
        standardButtons: Controls.Dialog.Close

        ColumnLayout {
            width: parent.width
            spacing: 8
            Controls.Label { text: "Веб-ссылка (Share URL):"; font.bold: true }
            Controls.TextField { Layout.fillWidth: true; readOnly: true; text: root.shareData.shareUrl || "" }
            Controls.Label { text: "Deep Link (kwhale://):"; font.bold: true }
            Controls.TextField { Layout.fillWidth: true; readOnly: true; text: root.shareData.deepLink || "" }
        }
    }

    // 7. Upload Track Dialog + XDG Desktop Portal FileDialogs
    Controls.Dialog {
        id: uploadTrackDialog
        anchors.centerIn: parent
        width: 440
        modal: true
        title: "Загрузить трек на сервер"
        standardButtons: Controls.Dialog.Ok | Controls.Dialog.Cancel
        onAccepted: bridge.upload_track_file(root.uploadSelectedFileUrl, uploadTitleField.text, uploadArtistField.text, uploadAlbumField.text)

        ColumnLayout {
            width: parent.width
            spacing: 10
            RowLayout {
                Layout.fillWidth: true
                Controls.Label {
                    text: root.uploadSelectedFileUrl.length > 0 ? root.uploadSelectedFileUrl : "Файл не выбран"
                    elide: Text.ElideLeft
                    Layout.fillWidth: true
                }
                Controls.Button {
                    text: "Выбрать файл…"
                    onClicked: audioUploadFileDialog.open()
                }
            }
            Controls.TextField { id: uploadTitleField; Layout.fillWidth: true; placeholderText: "Название трека (необязательно)" }
            Controls.TextField { id: uploadArtistField; Layout.fillWidth: true; placeholderText: "Артист (необязательно)" }
            Controls.TextField { id: uploadAlbumField; Layout.fillWidth: true; placeholderText: "Альбом (необязательно)" }
        }
    }

    FileDialog {
        id: audioUploadFileDialog
        title: "Выберите аудиофайл"
        nameFilters: ["Аудиофайлы (*.mp3 *.flac *.m4a *.ogg *.opus *.wav *.aac)"]
        onAccepted: root.uploadSelectedFileUrl = selectedFile.toString()
    }

    FileDialog {
        id: playlistCoverFileDialog
        title: "Выберите обложку плейлиста"
        nameFilters: ["Изображения (*.png *.jpg *.jpeg *.webp)"]
        onAccepted: bridge.upload_current_playlist_cover(selectedFile.toString())
    }

    // Toast notification pill
    Rectangle {
        id: toastBanner
        visible: toastTimer.running && bridge.toast_message.length > 0
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 100
        anchors.horizontalCenter: parent.horizontalCenter
        width: Math.min(parent.width - 40, toastLbl.implicitWidth + 36)
        height: 40
        radius: 20
        color: bridge.color_primary_container
        border.color: bridge.color_primary
        border.width: 1

        Controls.Label {
            id: toastLbl
            anchors.centerIn: parent
            text: bridge.toast_message
            color: bridge.color_on_primary_container
            font.bold: true
        }

        Timer {
            id: toastTimer
            interval: 3200
            repeat: false
        }
    }
}
