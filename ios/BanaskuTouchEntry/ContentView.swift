import SwiftUI
import UniformTypeIdentifiers
import PassKit

struct ContentView: View {
    @EnvironmentObject private var store: AppStore
    @State private var isImportingWalletPass = false
    @State private var isExportingWalletPass = false
    @State private var walletPassDocument: WalletPassDocument?
    @State private var isShowingWalletPass = false
    @State private var walletPassController: PKAddPassesViewController?

    var body: some View {
        TabView {
            NavigationStack { workflow }.tabItem { Label("検証", systemImage: "checkmark.shield") }
            NavigationStack { sessionLog }.tabItem { Label("ログ", systemImage: "list.bullet.rectangle") }
            NavigationStack { settings }.tabItem { Label("設定", systemImage: "slider.horizontal.3") }
        }
        .alert("お知らせ", isPresented: Binding(get: { store.alertMessage != nil }, set: { if !$0 { store.alertMessage = nil } })) {
            Button("閉じる", role: .cancel) { store.alertMessage = nil }
        } message: { Text(store.alertMessage ?? "") }
        .fileExporter(isPresented: $store.isExporting, document: ExportDocument(payload: store.exportPayload()), contentType: .json, defaultFilename: "banasku-session-\(store.sessionID.prefix(8))") { result in
            if case .failure(let error) = result { store.alertMessage = "エクスポートできませんでした: \(error.localizedDescription)" }
        }
        .fileExporter(isPresented: $isExportingWalletPass, document: walletPassDocument, contentType: UTType(filenameExtension: "pkpass") ?? .data, defaultFilename: "banasku-test.pkpass") { result in
            switch result {
            case .success:
                store.alertMessage = "Walletパスを書き出しました。Simulatorでは保存した.pkpassをSimulatorへドラッグして追加してください。"
            case .failure(let error):
                store.alertMessage = "Walletパスを書き出せませんでした: \(error.localizedDescription)"
            }
            walletPassDocument = nil
        }
        .fileImporter(
            isPresented: $isImportingWalletPass,
            allowedContentTypes: [UTType(filenameExtension: "pkpass", conformingTo: .data) ?? .data],
            allowsMultipleSelection: false,
            onCompletion: handleWalletPassImport
        )
        .sheet(isPresented: $isShowingWalletPass, onDismiss: { walletPassController = nil }) {
            if let walletPassController {
                WalletPassAddSheet(controller: walletPassController)
            }
        }
    }

    private var workflow: some View {
        ScrollView {
            VStack(spacing: 16) {
                modeBanner
                Group {
                    memberCard
                    credentialCard
                    reservationCard
                    registrationCard
                }
                .disabled(!store.hasRequiredServiceCredentials)
                disclaimer
            }
            .padding(16)
        }
        .background(Color(uiColor: .systemGroupedBackground))
        .navigationTitle("タッチ入場 検証")
        .toolbar { ToolbarItem(placement: .topBarTrailing) { if store.isBusy { ProgressView() } } }
    }

    private var modeBanner: some View {
        HStack(spacing: 12) {
            Image(systemName: store.mode == .mock ? "wrench.and.screwdriver.fill" : "cloud.fill")
                .font(.title2).foregroundStyle(.white)
            VStack(alignment: .leading, spacing: 3) {
                Text(store.modeLabel).font(.caption.bold())
                Text(store.mode == .mock
                     ? "画面上の模擬操作です"
                     : (store.hasRequiredServiceCredentials ? "設定したAPIへリクエストします" : "Bearer token必須 · 設定で入力して適用してください"))
                    .font(.caption).opacity(0.9)
            }
            Spacer(minLength: 0)
        }
        .foregroundStyle(.white)
        .padding(16)
        .background(store.mode == .mock || !store.hasRequiredServiceCredentials ? Color.orange : Color.blue, in: RoundedRectangle(cornerRadius: 16))
    }

    private var memberCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            sectionHeading("テスト会員", subtitle: "本番会員データは使用しません", icon: "person.crop.circle")
            Picker("会員", selection: $store.member) {
                ForEach(TestMember.samples) { member in Text("\(member.name) · \(member.memberNumber)").tag(member) }
            }
            .pickerStyle(.menu)
            .accessibilityLabel("テスト会員を選択")
            .disabled(store.mode == .api || !store.canChangeWorkflowContext)
            if store.mode == .mock {
                Text("会員を変更すると、この会員の資格情報・予約・登録・認可結果・ログを消去します。")
                    .font(.caption).foregroundStyle(.secondary)
            }
            if store.mode == .api {
                Text("APIモードの会員はBearer tokenでサーバー側が決定します。この選択は認証先に影響しません。")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .cardStyle()
    }

    private var credentialCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            sectionHeading("入場資格情報", subtitle: store.credential?.walletPassURL == nil ? "API署名設定があればWalletテストカードを発行できます" : "署名済みApple Walletテストカードを取得できます", icon: "key.horizontal")
            HStack {
                Label(store.credentialState == .notIssued ? (store.mode == .mock ? "モック発行可能" : "API発行先を設定可能") : store.credentialState.label, systemImage: "checkmark.seal")
                    .font(.subheadline)
                Spacer()
                Button(store.mode == .mock ? "モック発行" : "サーバー発行を試す") { Task { await store.issueCredential() } }
                    .buttonStyle(.borderedProminent).disabled(!store.canIssueCredential)
            }
            if let item = store.credential {
                Text("参照ID: …\(item.id.suffix(6))")
                    .font(.caption.monospaced()).foregroundStyle(.secondary)
                Text(item.providerLabel).font(.caption).foregroundStyle(.secondary)
                if item.walletPassURL != nil && item.state == .issued {
                    Button {
                        Task {
                            guard let data = await store.fetchWalletPass() else { return }
                            handleWalletPassData(data)
                        }
                    } label: {
                        Label("Apple Walletへ追加", systemImage: "wallet.pass")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(store.isBusy || store.isExporting)
                }
            } else if store.mode == .api {
                Label("Wallet署名設定: 発行後にAPI応答で確認", systemImage: "exclamationmark.triangle")
                    .font(.caption).foregroundStyle(.orange)
            }
        }
        .cardStyle()
    }

    private func handleWalletPassImport(_ result: Result<[URL], Error>) {
        switch result {
        case .success(let urls):
            guard let url = urls.first else { return }
            let hasSecurityScope = url.startAccessingSecurityScopedResource()
            defer { if hasSecurityScope { url.stopAccessingSecurityScopedResource() } }
            do {
                handleWalletPassData(try Data(contentsOf: url))
            } catch {
                store.alertMessage = "署名済みApple Walletパスを読み込めませんでした。Pass Type ID証明書で署名した.pkpassか確認してください。"
            }
        case .failure:
            store.alertMessage = "Walletパスを選択できませんでした。もう一度お試しください。"
        }
    }

    private func handleWalletPassData(_ data: Data) {
        do {
            let pass = try PKPass(data: data)
            guard PKAddPassesViewController.canAddPasses() else {
                walletPassDocument = WalletPassDocument(data: data)
                isExportingWalletPass = true
                return
            }
            guard let controller = PKAddPassesViewController(pass: pass) else {
                store.alertMessage = "Walletの追加画面を準備できませんでした。パス形式を確認してください。"
                return
            }
            walletPassController = controller
            isShowingWalletPass = true
        } catch {
            store.alertMessage = "署名済みApple Walletパスを読み込めませんでした。Pass Type ID証明書で署名した.pkpassか確認してください。"
        }
    }

    private var reservationCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            sectionHeading("テスト予約", subtitle: "有効時間は開始を含み終了を含みません [start, end)", icon: "calendar")
            TextField("ゲートID (テスト値)", text: $store.gateID).textFieldStyle(.roundedBorder).textInputAutocapitalization(.never)
            DatePicker("開始", selection: $store.startsAt, displayedComponents: [.date, .hourAndMinute])
            DatePicker("終了", selection: $store.endsAt, displayedComponents: [.date, .hourAndMinute])
            if let reservation = store.reservation {
                statusRow("予約", value: "\(reservation.status == "active" ? "有効" : "取消済み") · \(AppStore.date(reservation.startsAt)) – \(AppStore.date(reservation.endsAt))", color: reservation.status == "active" ? .green : .secondary)
                Text("予約ID: …\(reservation.id.suffix(6))").font(.caption.monospaced()).foregroundStyle(.secondary)
                if reservation.status == "active" {
                    HStack {
                        Button { Task { await store.updateReservation() } } label: { Label("日時を更新", systemImage: "calendar.badge.clock").frame(maxWidth: .infinity) }
                            .buttonStyle(.bordered).disabled(store.isBusy)
                        Button(role: .destructive) { Task { await store.cancelReservation() } } label: { Label("予約を取消", systemImage: "calendar.badge.minus").frame(maxWidth: .infinity) }
                            .buttonStyle(.bordered).disabled(store.isBusy)
                    }
                } else {
                    createReservationButton
                }
            } else {
                createReservationButton
            }
        }
        .cardStyle()
        .environment(\.timeZone, AppStore.displayTimeZone)
    }

    private var createReservationButton: some View {
        Button { Task { await store.createReservation() } } label: {
            Label("テスト予約を作成", systemImage: "calendar.badge.plus").frame(maxWidth: .infinity)
        }
        .buttonStyle(.bordered).disabled(store.isBusy)
    }

    private var registrationCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            sectionHeading("オンライン登録", subtitle: "登録状態の検証であり、ゲート操作ではありません", icon: "antenna.radiowaves.left.and.right")
            statusRow("登録状態", value: store.registration?.state.label ?? "未登録", color: store.registration?.state == .registered ? .green : .secondary)
            HStack {
                Button { Task { await store.register() } } label: { Label("登録", systemImage: "arrow.up.circle.fill").frame(maxWidth: .infinity) }
                    .buttonStyle(.borderedProminent).disabled(store.isBusy || store.credential?.state != .issued || store.reservation?.status != "active" || !store.canCreateRegistration)
                Button { Task { await store.refreshRegistration() } } label: { Label("状態取得", systemImage: "arrow.clockwise").frame(maxWidth: .infinity) }
                    .buttonStyle(.bordered).disabled(store.isBusy || store.registration == nil)
            }
            if store.registration?.state == .failed {
                Button { Task { await store.retryRegistration() } } label: {
                    Label("失敗した登録を再試行", systemImage: "arrow.clockwise.circle").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent).disabled(store.isBusy)
            }
            Button(role: .destructive) { Task { await store.cancelRegistration() } } label: {
                Label("登録を取消", systemImage: "xmark.circle").frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered).disabled(store.isBusy || store.registration == nil || store.registration?.state == .cancelled)
            if let registration = store.registration {
                Text("登録参照ID: …\(registration.id.suffix(6))").font(.caption.monospaced()).foregroundStyle(.secondary)
                Text("最終更新: \(AppStore.date(registration.updatedAt))").font(.caption).foregroundStyle(.secondary)
            }
            Divider()
            HStack {
                Text("時間帯認可").font(.subheadline.bold())
                Spacer()
                Button("現在時刻を確認") { Task { await store.checkAuthorization() } }
                    .buttonStyle(.bordered).disabled(store.isBusy || store.registration == nil)
            }
            if let result = store.lastAuthorization {
                statusRow("判定", value: result.allowed ? "許可 · \(result.reason)" : "拒否 · \(result.reason)", color: result.allowed ? .green : .orange)
                Text("\(AppStore.date(result.evaluatedAt)) · gate_applied=false · physical_unlock_confirmed=false")
                    .font(.caption).foregroundStyle(.secondary)
            }
            if store.mode == .mock, let reservation = store.reservation, store.registration != nil {
                Text("モック時刻境界の確認").font(.caption.bold()).foregroundStyle(.secondary)
                HStack {
                    boundaryButton("開始前", time: reservation.startsAt.addingTimeInterval(-1))
                    boundaryButton("開始時", time: reservation.startsAt)
                    boundaryButton("終了直前", time: reservation.endsAt.addingTimeInterval(-1))
                    boundaryButton("終了時", time: reservation.endsAt)
                }
            }
        }
        .cardStyle()
    }

    private func boundaryButton(_ title: String, time: Date) -> some View {
        Button(title) { Task { await store.checkAuthorization(at: time) } }
            .font(.caption)
            .buttonStyle(.bordered)
            .disabled(store.isBusy || store.registration == nil)
    }

    private var disclaimer: some View {
        Label("このアプリはタッチ送信や物理解錠を行わず、その成功も示しません。", systemImage: "hand.raised.fill")
            .font(.footnote).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 4)
    }

    private var sessionLog: some View {
        List {
            Section {
                LabeledContent("セッションID", value: String(store.sessionID.prefix(8)))
                LabeledContent("モード", value: store.mode.rawValue)
                Button { Task { await store.fetchServerEvents() } } label: { Label("APIイベントを取得", systemImage: "arrow.down.doc") }
                    .disabled(store.isBusy || store.mode == .mock || !store.hasRequiredServiceCredentials)
                Button {
                    store.isExporting = true
                } label: { Label("サニタイズ済みJSONを書き出す", systemImage: "square.and.arrow.up") }
                    .disabled(store.isBusy || store.isExporting)
            } header: { Text("セッション") } footer: { Text("エクスポートにはトークン・資格情報ID・予約/登録ID・サーバー詳細を含めません。") }
            Section("イベント · \(store.events.count)") {
                if store.events.isEmpty { ContentUnavailableView("ログはまだありません", systemImage: "list.bullet.rectangle", description: Text("検証操作を行うとこのセッションに記録されます。")) }
                ForEach(store.events) { event in
                    VStack(alignment: .leading, spacing: 5) {
                        HStack { Text(event.action).font(.subheadline.bold()); Spacer(); Text(event.result).font(.caption).foregroundStyle(.secondary) }
                        Text(event.detail).font(.caption).foregroundStyle(.secondary)
                        Text(AppStore.date(event.timestamp)).font(.caption2).foregroundStyle(.tertiary)
                    }.padding(.vertical, 3)
                }
            }
        }
        .navigationTitle("監査 / セッションログ")
    }

    private var settings: some View {
        Form {
            Section("接続モード") {
                Picker("モード", selection: $store.mode) {
                    ForEach(ServiceMode.allCases) { mode in Text(mode.rawValue).tag(mode) }
                }.pickerStyle(.segmented)
                    .disabled(!store.canChangeWorkflowContext)
                Text(store.mode == .mock
                     ? "すべてローカルの模擬データです。モードを切り替えると資格情報・予約・登録・認可結果・ログを消去します。"
                     : "Sandbox APIへBearer認証で接続します。モードを切り替えると保存済みの資格情報・予約・登録・認可結果・ログと入力中のtokenを消去します。APIが返す資格情報IDは参照情報で、Apple資格情報ではありません。")
                    .font(.footnote).foregroundStyle(.secondary)
                if store.mode == .api {
                    Button { Task { await store.restoreServerState() } } label: {
                        Label("保存済み状態をサーバーと同期", systemImage: "arrow.triangle.2.circlepath")
                    }
                    .disabled(store.isBusy || !store.hasRequiredServiceCredentials)
                    Text("API操作にはBearer tokenが必須です。アプリ再起動後はtokenを認証してから、保存済みIDの状態を同期してください。")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            Section("API設定") {
                LabeledContent("Sandbox API", value: AppStore.apiBaseURL)
                SecureField("Bearer token（APIモードで必須）", text: $store.bearerTokenDraft).textInputAutocapitalization(.never).autocorrectionDisabled()
                    .disabled(!store.canChangeWorkflowContext)
                Button("Bearer tokenを適用・認証") { Task { await store.applyBearerToken() } }
                    .disabled(store.mode != .api || store.isBusy || store.isExporting || store.bearerTokenDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                Text("APIモードではtokenを入力して認証するまで操作できません。tokenのアカウントが変わると前の状態とログを消去します。同じアカウントなら、アプリ再起動後も保存済みIDを同期できます。tokenはメモリー上だけで使い、端末保存・ログ・エクスポートには含めません。API接続先はアプリに固定されています。")
                    .font(.footnote).foregroundStyle(.secondary)
            }
            Section("Apple Walletパスの追加テスト") {
                Button {
                    isImportingWalletPass = true
                } label: {
                    Label("署名済み.pkpassをWalletへ追加", systemImage: "wallet.pass")
                }
                .disabled(store.isBusy || store.isExporting)
                Text("テスト用の署名済みパスをファイルから読み込み、Appleの追加確認を表示します。APIの参照ID発行やNFC・ゲート提示とは別の操作です。")
                    .font(.footnote).foregroundStyle(.secondary)
            }
            Section("実機連携の状態") {
                statusRow("Apple発行 entitlement", value: CredentialState.unavailableEntitlement.label, color: .orange)
                statusRow("iPhoneデバイス対応", value: CredentialState.unavailableDevice.label + " (未検査)", color: .secondary)
                statusRow("Apple資格情報プロバイダー", value: CredentialState.unavailableProvider.label, color: .secondary)
                statusRow("検証用プロバイダー", value: store.mode == .mock ? "モック" : "設定したAPI参照", color: .secondary)
                statusRow("NFC・ゲート動作", value: "未実装・未検証", color: .secondary)
                Text("利用可能な entitlement、デバイス、プロバイダーがない場合は発行処理を行わず、その状態を区別して表示します。")
                    .font(.footnote).foregroundStyle(.secondary)
            }
            Section("失敗・再試行シナリオ") {
                Toggle("次の登録を失敗させる", isOn: $store.simulateRegistrationFailure)
                    .disabled(!store.canSimulateRegistrationFailure)
                Text(store.mode == .mock
                     ? "モック登録を失敗状態にして、状態表示と再試行操作を確認します。"
                     : "localhost APIで ALLOW_TIME_SIMULATION=true の場合に限り有効です。")
                    .font(.footnote).foregroundStyle(.secondary)
            }
        }
        .navigationTitle("設定")
    }

    private func sectionHeading(_ title: String, subtitle: String, icon: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon).font(.title3).foregroundStyle(Color.accentColor).frame(width: 24)
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(.headline)
                Text(subtitle).font(.caption).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
    }

    private func statusRow(_ title: String, value: String, color: Color) -> some View {
        HStack { Text(title).font(.subheadline); Spacer(); Text(value).font(.subheadline.weight(.medium)).foregroundStyle(color).multilineTextAlignment(.trailing) }
    }
}

private struct WalletPassDocument: FileDocument {
    static var readableContentTypes: [UTType] { [UTType(filenameExtension: "pkpass") ?? .data] }
    let data: Data

    init(data: Data) { self.data = data }

    init(configuration: ReadConfiguration) throws {
        guard let data = configuration.file.regularFileContents else { throw CocoaError(.fileReadCorruptFile) }
        self.data = data
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: data)
    }
}

private struct WalletPassAddSheet: UIViewControllerRepresentable {
    let controller: PKAddPassesViewController
    @Environment(\.dismiss) private var dismiss

    func makeCoordinator() -> Coordinator {
        Coordinator { dismiss() }
    }

    func makeUIViewController(context: Context) -> PKAddPassesViewController {
        controller.delegate = context.coordinator
        return controller
    }

    func updateUIViewController(_ uiViewController: PKAddPassesViewController, context: Context) {}

    final class Coordinator: NSObject, PKAddPassesViewControllerDelegate {
        private let onFinish: () -> Void

        init(onFinish: @escaping () -> Void) {
            self.onFinish = onFinish
        }

        func addPassesViewControllerDidFinish(_ controller: PKAddPassesViewController) {
            onFinish()
        }
    }
}

private struct ExportDocument: FileDocument {
    static var readableContentTypes: [UTType] { [.json] }
    private var data: Data

    init(payload: ExportPayload? = nil) {
        let value = payload ?? ExportPayload(exportedAt: .now, sessionID: "", mode: "", member: "", credential: "", reservation: "", registration: "", events: [], note: "")
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        self.data = (try? encoder.encode(value)) ?? Data("{}".utf8)
    }

    init(configuration: ReadConfiguration) throws { data = configuration.file.regularFileContents ?? Data() }
    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper { FileWrapper(regularFileWithContents: data) }
}

private extension View {
    func cardStyle() -> some View {
        padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 18))
    }
}
