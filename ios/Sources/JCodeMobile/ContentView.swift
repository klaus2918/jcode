import SwiftUI
import JCodeKit

#if canImport(UIKit)
import UIKit
#endif

// MARK: - Root

struct RootView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        Group {
            if model.savedServers.isEmpty && model.connectionState == .disconnected {
                OnboardingView()
            } else {
                MainView()
            }
        }
        .preferredColorScheme(.dark)
        .task {
            await model.loadSavedServers()
            if model.selectedServer != nil && model.connectionState == .disconnected {
                await model.connectSelected()
            }
        }
    }
}

// MARK: - Onboarding

struct OnboardingView: View {
    @EnvironmentObject private var model: AppModel
    @State private var showQRScanner = false
    @State private var showManualEntry = false

    var body: some View {
        ZStack {
            JC.Colors.background.ignoresSafeArea()

            ScrollView {
                VStack(spacing: JC.Spacing.xxl) {
                    Spacer().frame(height: 60)

                    VStack(spacing: JC.Spacing.lg) {
                        TerminalPrompt()
                            .frame(width: 80, height: 80)

                        Text("jcode")
                            .font(JC.Fonts.largeTitle)
                            .foregroundStyle(JC.Colors.textPrimary)

                        Text("Your AI coding assistant,\nright in your pocket.")
                            .font(JC.Fonts.body)
                            .foregroundStyle(JC.Colors.textSecondary)
                            .multilineTextAlignment(.center)
                    }

                    Spacer().frame(height: 20)

                    VStack(spacing: JC.Spacing.lg) {
                        Button {
                            showQRScanner = true
                        } label: {
                            HStack(spacing: JC.Spacing.md) {
                                Image(systemName: "qrcode.viewfinder")
                                    .font(.system(size: 24))
                                Text("Scan QR Code")
                                    .font(JC.Fonts.headline)
                            }
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, JC.Spacing.xl)
                        }
                        .buttonStyle(AccentButton())

                        Text("Run **jcode pair** on your computer\nto generate a QR code.")
                            .font(JC.Fonts.callout)
                            .foregroundStyle(JC.Colors.textSecondary)
                            .multilineTextAlignment(.center)
                    }
                    .padding(.horizontal, JC.Spacing.xxl)

                    if let error = model.errorMessage {
                        HStack(spacing: JC.Spacing.sm) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .font(.caption)
                            Text(error)
                                .font(JC.Fonts.caption)
                        }
                        .foregroundStyle(JC.Colors.destructive)
                        .padding(.horizontal, JC.Spacing.xxl)
                    }

                    if let status = model.statusMessage {
                        HStack(spacing: JC.Spacing.sm) {
                            Image(systemName: "checkmark.circle.fill")
                                .font(.caption)
                            Text(status)
                                .font(JC.Fonts.caption)
                        }
                        .foregroundStyle(JC.Colors.accent)
                        .padding(.horizontal, JC.Spacing.xxl)
                    }

                    Spacer().frame(height: 20)

                    VStack(spacing: JC.Spacing.md) {
                        Button {
                            withAnimation(JC.Animation.smooth) {
                                showManualEntry.toggle()
                            }
                        } label: {
                            HStack(spacing: JC.Spacing.xs) {
                                Text("Connect manually")
                                    .font(JC.Fonts.caption)
                                Image(systemName: showManualEntry ? "chevron.up" : "chevron.down")
                                    .font(.system(size: 10, weight: .semibold))
                            }
                            .foregroundStyle(JC.Colors.textTertiary)
                        }
                        .buttonStyle(.plain)

                        if showManualEntry {
                            ManualEntryFields()
                                .transition(.opacity.combined(with: .move(edge: .top)))
                        }
                    }
                    .padding(.horizontal, JC.Spacing.xxl)
                    .padding(.bottom, JC.Spacing.xxxl)
                }
            }
        }
        .sheet(isPresented: $showQRScanner) {
            QRScannerView(isPresented: $showQRScanner) { host, port, code in
                model.hostInput = host
                model.portInput = String(port)
                model.pairCodeInput = code
                Task { await model.pairAndSave() }
            }
        }
    }
}

struct ManualEntryFields: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: JC.Spacing.md) {
            JCTextField(
                label: "Host",
                placeholder: "e.g. my-macbook",
                text: $model.hostInput,
                icon: "server.rack"
            )

            JCTextField(
                label: "Port",
                placeholder: "7643",
                text: $model.portInput,
                icon: "number",
                keyboardType: .numberPad
            )

            JCTextField(
                label: "Pair Code",
                placeholder: "6-digit code from jcode pair",
                text: $model.pairCodeInput,
                icon: "key.fill"
            )

            JCTextField(
                label: "Device Name",
                placeholder: "My iPhone",
                text: $model.deviceNameInput,
                icon: "iphone"
            )

            Button {
                Task { await model.pairAndSave() }
            } label: {
                HStack(spacing: JC.Spacing.sm) {
                    Image(systemName: "link")
                    Text("Pair & Connect")
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(AccentButton())
        }
    }
}

// MARK: - Terminal Prompt Animation

struct TerminalPrompt: View {
    @State private var cursorVisible = true

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: JC.Radius.lg, style: .continuous)
                .fill(JC.Colors.surface)
                .overlay(
                    RoundedRectangle(cornerRadius: JC.Radius.lg, style: .continuous)
                        .stroke(JC.Colors.border, lineWidth: 1)
                )

            HStack(spacing: 2) {
                Text("j")
                    .font(.system(size: 32, weight: .bold, design: .monospaced))
                    .foregroundStyle(JC.Colors.accent)
                Text(">")
                    .font(.system(size: 32, weight: .bold, design: .monospaced))
                    .foregroundStyle(JC.Colors.textSecondary)
                Rectangle()
                    .fill(JC.Colors.accent)
                    .frame(width: 3, height: 28)
                    .opacity(cursorVisible ? 1 : 0)
            }
        }
        .onAppear {
            withAnimation(.easeInOut(duration: 0.6).repeatForever(autoreverses: true)) {
                cursorVisible.toggle()
            }
        }
    }
}

// MARK: - Custom Text Field

struct JCTextField: View {
    let label: String
    let placeholder: String
    @Binding var text: String
    var icon: String = ""
    var keyboardType: UIKeyboardType = .default

    @FocusState private var isFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: JC.Spacing.xs) {
            Text(label)
                .font(JC.Fonts.caption)
                .foregroundStyle(JC.Colors.textTertiary)

            HStack(spacing: JC.Spacing.sm) {
                if !icon.isEmpty {
                    Image(systemName: icon)
                        .font(.system(size: 14))
                        .foregroundStyle(isFocused ? JC.Colors.accent : JC.Colors.textTertiary)
                        .frame(width: 20)
                }

                TextField(placeholder, text: $text)
                    .font(JC.Fonts.body)
                    .foregroundStyle(JC.Colors.textPrimary)
                    .focused($isFocused)
                    .keyboardType(keyboardType)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled(true)
            }
            .padding(.horizontal, JC.Spacing.md)
            .padding(.vertical, JC.Spacing.md)
            .background(JC.Colors.surfaceElevated)
            .clipShape(RoundedRectangle(cornerRadius: JC.Radius.sm, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: JC.Radius.sm, style: .continuous)
                    .stroke(isFocused ? JC.Colors.borderFocused : JC.Colors.border, lineWidth: 1)
            )
            .animation(JC.Animation.quick, value: isFocused)
        }
    }
}

// MARK: - Main App (Connected State)

struct MainView: View {
    @EnvironmentObject private var model: AppModel
    @State private var showSettings = false

    var body: some View {
        NavigationStack {
            ZStack {
                JC.Colors.background.ignoresSafeArea()

                VStack(spacing: 0) {
                    ChatHeaderBar(showSettings: $showSettings)
                    Divider().overlay(JC.Colors.border)
                    ChatMessageList()
                    Divider().overlay(JC.Colors.border)
                    ChatInputBar()
                }
            }
            .sheet(isPresented: $showSettings) {
                SettingsSheet()
            }
        }
    }
}

// MARK: - Chat Header

struct ChatHeaderBar: View {
    @EnvironmentObject private var model: AppModel
    @Binding var showSettings: Bool

    var body: some View {
        HStack(spacing: JC.Spacing.md) {
            Button { showSettings = true } label: {
                HStack(spacing: JC.Spacing.sm) {
                    StatusDot(
                        color: statusColor,
                        animated: model.connectionState == .connecting
                    )

                    VStack(alignment: .leading, spacing: 1) {
                        Text(model.serverName.isEmpty ? "jcode" : model.serverName)
                            .font(JC.Fonts.headline)
                            .foregroundStyle(JC.Colors.textPrimary)
                        Text(subtitle)
                            .font(JC.Fonts.monoCaption)
                            .foregroundStyle(JC.Colors.textTertiary)
                    }
                }
            }
            .buttonStyle(.plain)

            Spacer()

            if model.isProcessing {
                TypingIndicator()
            }

            if !model.modelName.isEmpty {
                PillBadge(text: shortModelName)
            }
        }
        .padding(.horizontal, JC.Spacing.lg)
        .padding(.vertical, JC.Spacing.md)
        .background(JC.Colors.surface)
    }

    private var statusColor: Color {
        switch model.connectionState {
        case .connected: JC.Colors.statusOnline
        case .connecting: JC.Colors.statusConnecting
        case .disconnected: JC.Colors.statusOffline
        }
    }

    private var subtitle: String {
        switch model.connectionState {
        case .connected:
            if !model.serverVersion.isEmpty {
                return model.serverVersion
            }
            return "Connected"
        case .connecting:
            return "Connecting..."
        case .disconnected:
            return "Offline"
        }
    }

    private var shortModelName: String {
        let name = model.modelName
        if let slash = name.lastIndex(of: "/") {
            return String(name[name.index(after: slash)...])
        }
        if name.count > 20 {
            return String(name.prefix(18)) + "..."
        }
        return name
    }
}

// MARK: - Typing Indicator

struct TypingIndicator: View {
    @State private var phase: Int = 0

    var body: some View {
        HStack(spacing: 3) {
            ForEach(0..<3) { i in
                Circle()
                    .fill(JC.Colors.accent)
                    .frame(width: 5, height: 5)
                    .scaleEffect(phase == i ? 1.3 : 0.7)
                    .opacity(phase == i ? 1.0 : 0.4)
            }
        }
        .onAppear {
            withAnimation(.easeInOut(duration: 0.4).repeatForever(autoreverses: true)) {
                phase = 1
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                withAnimation(.easeInOut(duration: 0.4).repeatForever(autoreverses: true)) {
                    phase = 2
                }
            }
        }
    }
}

// MARK: - Chat Message List

struct ChatMessageList: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: JC.Spacing.lg) {
                    if model.messages.isEmpty && model.connectionState == .connected {
                        emptyState
                    }

                    ForEach(model.messages) { message in
                        MessageRow(message: message)
                            .id(message.id)
                    }
                }
                .padding(JC.Spacing.lg)
            }
            .background(JC.Colors.background)
            .onChange(of: model.messages.count) {
                scrollToBottom(proxy)
            }
            .onChange(of: model.messages.last?.text) {
                scrollToBottom(proxy)
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: JC.Spacing.md) {
            Spacer().frame(height: 80)

            Image(systemName: "bubble.left.and.bubble.right")
                .font(.system(size: 40))
                .foregroundStyle(JC.Colors.textTertiary)

            Text("Start a conversation")
                .font(JC.Fonts.headline)
                .foregroundStyle(JC.Colors.textSecondary)

            Text("Type a message below to get started.")
                .font(JC.Fonts.callout)
                .foregroundStyle(JC.Colors.textTertiary)
        }
    }

    private func scrollToBottom(_ proxy: ScrollViewProxy) {
        if let id = model.messages.last?.id {
            withAnimation(JC.Animation.quick) {
                proxy.scrollTo(id, anchor: .bottom)
            }
        }
    }
}

// MARK: - Message Row

struct MessageRow: View {
    let message: AppModel.ChatEntry

    var body: some View {
        VStack(alignment: alignment, spacing: JC.Spacing.sm) {
            HStack(spacing: JC.Spacing.xs) {
                if message.role == .user {
                    Spacer()
                }

                Text(roleLabel)
                    .font(JC.Fonts.caption2)
                    .foregroundStyle(JC.Colors.textTertiary)

                if message.role != .user {
                    Spacer()
                }
            }

            if !message.images.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: JC.Spacing.sm) {
                        ForEach(Array(message.images.enumerated()), id: \.offset) { _, pair in
                            if let data = Data(base64Encoded: pair.1),
                               let uiImage = UIImage(data: data) {
                                Image(uiImage: uiImage)
                                    .resizable()
                                    .aspectRatio(contentMode: .fit)
                                    .frame(maxWidth: 200, maxHeight: 200)
                                    .clipShape(RoundedRectangle(cornerRadius: JC.Radius.sm))
                            }
                        }
                    }
                }
            }

            if message.role == .assistant && !message.text.isEmpty {
                MarkdownText(text: message.text)
                    .padding(JC.Spacing.md)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(JC.Colors.assistantBubble)
                    .clipShape(RoundedRectangle(cornerRadius: JC.Radius.md, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: JC.Radius.md, style: .continuous)
                            .stroke(JC.Colors.border, lineWidth: 1)
                    )
            } else if message.role == .system {
                HStack(spacing: JC.Spacing.sm) {
                    Image(systemName: "info.circle.fill")
                        .font(.caption)
                        .foregroundStyle(JC.Colors.statusConnecting)
                    Text(message.text)
                        .font(JC.Fonts.callout)
                        .foregroundStyle(JC.Colors.textSecondary)
                }
                .padding(JC.Spacing.md)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(JC.Colors.systemBubble)
                .clipShape(RoundedRectangle(cornerRadius: JC.Radius.md, style: .continuous))
            } else if !message.text.isEmpty || message.images.isEmpty {
                Text(message.text.isEmpty ? "..." : message.text)
                    .font(JC.Fonts.body)
                    .textSelection(.enabled)
                    .foregroundStyle(JC.Colors.textPrimary)
                    .padding(JC.Spacing.md)
                    .frame(maxWidth: 520, alignment: .leading)
                    .background(JC.Colors.userBubble)
                    .clipShape(RoundedRectangle(cornerRadius: JC.Radius.md, style: .continuous))
            }

            if !message.toolCalls.isEmpty {
                VStack(spacing: JC.Spacing.sm) {
                    ForEach(message.toolCalls, id: \.id) { tool in
                        ToolCardView(tool: tool)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: message.role == .user ? .trailing : .leading)
    }

    private var alignment: HorizontalAlignment {
        message.role == .user ? .trailing : .leading
    }

    private var roleLabel: String {
        switch message.role {
        case .assistant: "jcode"
        case .system: "System"
        case .user: "You"
        }
    }
}

// MARK: - Tool Card

struct ToolCardView: View {
    let tool: ToolCallInfo
    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                withAnimation(JC.Animation.standard) {
                    isExpanded.toggle()
                }
            } label: {
                HStack(spacing: JC.Spacing.sm) {
                    Image(systemName: toolIcon)
                        .font(.system(size: 12))
                        .foregroundStyle(stateColor)
                        .frame(width: 20)

                    Text(tool.name)
                        .font(JC.Fonts.mono)
                        .foregroundStyle(JC.Colors.textPrimary)
                        .lineLimit(1)

                    Spacer()

                    if tool.state == .executing || tool.state == .streaming {
                        ProgressView()
                            .controlSize(.mini)
                            .tint(stateColor)
                    }

                    PillBadge(text: stateLabel, color: stateColor)

                    Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(JC.Colors.textTertiary)
                }
                .padding(.horizontal, JC.Spacing.md)
                .padding(.vertical, JC.Spacing.sm + 2)
            }
            .buttonStyle(.plain)

            if isExpanded {
                Divider().overlay(JC.Colors.border)

                VStack(alignment: .leading, spacing: JC.Spacing.sm) {
                    if !tool.input.isEmpty {
                        VStack(alignment: .leading, spacing: JC.Spacing.xs) {
                            Text("INPUT")
                                .font(JC.Fonts.monoCaption)
                                .foregroundStyle(JC.Colors.textTertiary)
                            Text(tool.input)
                                .font(JC.Fonts.monoSmall)
                                .foregroundStyle(JC.Colors.textSecondary)
                                .lineLimit(15)
                                .textSelection(.enabled)
                        }
                    }

                    if let output = tool.output, !output.isEmpty {
                        VStack(alignment: .leading, spacing: JC.Spacing.xs) {
                            Text("OUTPUT")
                                .font(JC.Fonts.monoCaption)
                                .foregroundStyle(JC.Colors.textTertiary)
                            Text(output)
                                .font(JC.Fonts.monoSmall)
                                .foregroundStyle(JC.Colors.textPrimary)
                                .lineLimit(15)
                                .textSelection(.enabled)
                        }
                    }

                    if let error = tool.error, !error.isEmpty {
                        HStack(spacing: JC.Spacing.xs) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .font(.caption2)
                            Text(error)
                                .font(JC.Fonts.monoSmall)
                        }
                        .foregroundStyle(JC.Colors.toolFailed)
                    }
                }
                .padding(JC.Spacing.md)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(JC.Colors.codeBackground)
            }
        }
        .background(JC.Colors.surface)
        .clipShape(RoundedRectangle(cornerRadius: JC.Radius.sm, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: JC.Radius.sm, style: .continuous)
                .stroke(JC.Colors.border, lineWidth: 1)
        )
    }

    private var toolIcon: String {
        switch tool.state {
        case .streaming: "arrow.down.circle"
        case .executing: "gearshape.2"
        case .done: "checkmark.circle.fill"
        case .failed: "xmark.circle.fill"
        }
    }

    private var stateLabel: String {
        switch tool.state {
        case .streaming: "streaming"
        case .executing: "running"
        case .done: "done"
        case .failed: "failed"
        }
    }

    private var stateColor: Color {
        switch tool.state {
        case .streaming: JC.Colors.toolStreaming
        case .executing: JC.Colors.toolRunning
        case .done: JC.Colors.toolDone
        case .failed: JC.Colors.toolFailed
        }
    }
}

// MARK: - Chat Input Bar

struct ChatInputBar: View {
    @EnvironmentObject private var model: AppModel
    @State private var showInterruptSheet = false
    @State private var interruptMessage = ""
    @State private var attachments: [ImageAttachment] = []
    @FocusState private var inputFocused: Bool

    var body: some View {
        VStack(spacing: JC.Spacing.sm) {
            if model.isProcessing {
                HStack(spacing: JC.Spacing.sm) {
                    Button {
                        Task { await model.cancelGeneration() }
                    } label: {
                        HStack(spacing: JC.Spacing.xs) {
                            Image(systemName: "stop.fill")
                                .font(.system(size: 10))
                            Text("Stop")
                                .font(JC.Fonts.caption)
                        }
                        .foregroundStyle(JC.Colors.destructive)
                        .padding(.horizontal, JC.Spacing.md)
                        .padding(.vertical, JC.Spacing.xs + 2)
                        .background(JC.Colors.destructive.opacity(0.12))
                        .clipShape(Capsule())
                    }
                    .buttonStyle(.plain)

                    Button {
                        showInterruptSheet = true
                    } label: {
                        HStack(spacing: JC.Spacing.xs) {
                            Image(systemName: "bolt.fill")
                                .font(.system(size: 10))
                            Text("Interrupt")
                                .font(JC.Fonts.caption)
                        }
                        .foregroundStyle(JC.Colors.statusConnecting)
                        .padding(.horizontal, JC.Spacing.md)
                        .padding(.vertical, JC.Spacing.xs + 2)
                        .background(JC.Colors.statusConnecting.opacity(0.12))
                        .clipShape(Capsule())
                    }
                    .buttonStyle(.plain)

                    Spacer()
                }
                .padding(.horizontal, JC.Spacing.xs)
                .transition(.move(edge: .bottom).combined(with: .opacity))
            }

            if !attachments.isEmpty {
                AttachmentStrip(attachments: $attachments)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }

            HStack(alignment: .bottom, spacing: JC.Spacing.sm) {
                PhotoPickerButton(attachments: $attachments)

                if UIImagePickerController.isSourceTypeAvailable(.camera) {
                    CameraButton(attachments: $attachments)
                }

                HStack(spacing: 0) {
                    TextField("Message jcode...", text: $model.draftMessage, axis: .vertical)
                        .font(JC.Fonts.body)
                        .foregroundStyle(JC.Colors.textPrimary)
                        .lineLimit(1...6)
                        .focused($inputFocused)
                        .padding(.horizontal, JC.Spacing.md)
                        .padding(.vertical, JC.Spacing.sm + 2)
                }
                .background(JC.Colors.surfaceElevated)
                .clipShape(RoundedRectangle(cornerRadius: JC.Radius.xl, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: JC.Radius.xl, style: .continuous)
                        .stroke(inputFocused ? JC.Colors.borderFocused : JC.Colors.border, lineWidth: 1)
                )

                Button {
                    let images = attachments.map { ($0.mediaType, $0.base64Data) }
                    attachments.removeAll()
                    Task { await model.sendDraft(images: images) }
                } label: {
                    Image(systemName: "arrow.up")
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(canSend ? JC.Colors.textOnAccent : JC.Colors.textTertiary)
                        .frame(width: 32, height: 32)
                        .background(canSend ? JC.Colors.accent : JC.Colors.surfaceElevated)
                        .clipShape(Circle())
                }
                .buttonStyle(.plain)
                .disabled(!canSend)
                .animation(JC.Animation.quick, value: canSend)
            }
        }
        .padding(.horizontal, JC.Spacing.md)
        .padding(.vertical, JC.Spacing.sm + 2)
        .background(JC.Colors.surface)
        .animation(JC.Animation.standard, value: model.isProcessing)
        .sheet(isPresented: $showInterruptSheet) {
            InterruptSheet(
                message: $interruptMessage,
                isPresented: $showInterruptSheet,
                onSend: { msg in
                    Task { await model.interruptAgent(msg) }
                }
            )
        }
    }

    private var canSend: Bool {
        model.connectionState == .connected &&
        (!model.draftMessage.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty)
    }
}

// MARK: - Interrupt Sheet

struct InterruptSheet: View {
    @Binding var message: String
    @Binding var isPresented: Bool
    let onSend: (String) -> Void

    var body: some View {
        NavigationStack {
            ZStack {
                JC.Colors.background.ignoresSafeArea()

                VStack(spacing: JC.Spacing.lg) {
                    TextField("What should the agent know?", text: $message, axis: .vertical)
                        .font(JC.Fonts.body)
                        .foregroundStyle(JC.Colors.textPrimary)
                        .lineLimit(2...6)
                        .padding(JC.Spacing.md)
                        .background(JC.Colors.surfaceElevated)
                        .clipShape(RoundedRectangle(cornerRadius: JC.Radius.sm))
                        .overlay(
                            RoundedRectangle(cornerRadius: JC.Radius.sm)
                                .stroke(JC.Colors.border, lineWidth: 1)
                        )

                    Spacer()
                }
                .padding(JC.Spacing.lg)
            }
            .navigationTitle("Interrupt Agent")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { isPresented = false }
                        .foregroundStyle(JC.Colors.textSecondary)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") {
                        let msg = message.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !msg.isEmpty else { return }
                        onSend(msg)
                        message = ""
                        isPresented = false
                    }
                    .foregroundStyle(JC.Colors.accent)
                    .disabled(message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
        .presentationDetents([.medium])
        .presentationBackground(JC.Colors.background)
    }
}

// MARK: - Settings Sheet

struct SettingsSheet: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var showQRScanner = false
    @State private var showAddServer = false

    var body: some View {
        NavigationStack {
            ZStack {
                JC.Colors.background.ignoresSafeArea()

                ScrollView {
                    VStack(spacing: JC.Spacing.xl) {
                        connectionSection
                        serversSection
                        sessionsSection
                        modelSection
                    }
                    .padding(JC.Spacing.lg)
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .foregroundStyle(JC.Colors.accent)
                }
            }
        }
        .presentationBackground(JC.Colors.background)
        .sheet(isPresented: $showQRScanner) {
            QRScannerView(isPresented: $showQRScanner) { host, port, code in
                model.hostInput = host
                model.portInput = String(port)
                model.pairCodeInput = code
                Task { await model.pairAndSave() }
            }
        }
        .sheet(isPresented: $showAddServer) {
            AddServerSheet(isPresented: $showAddServer)
        }
    }

    private var connectionSection: some View {
        VStack(alignment: .leading, spacing: JC.Spacing.md) {
            SectionHeader(title: "Connection")

            HStack(spacing: JC.Spacing.md) {
                StatusDot(
                    color: statusColor,
                    animated: model.connectionState == .connecting
                )

                VStack(alignment: .leading, spacing: 2) {
                    Text(statusText)
                        .font(JC.Fonts.headline)
                        .foregroundStyle(JC.Colors.textPrimary)
                    if let server = model.selectedServer {
                        Text("\(server.host):\(server.port)")
                            .font(JC.Fonts.monoSmall)
                            .foregroundStyle(JC.Colors.textTertiary)
                    }
                }

                Spacer()

                if model.connectionState == .connected {
                    Button {
                        Task { await model.disconnect() }
                    } label: {
                        Text("Disconnect")
                            .font(JC.Fonts.caption)
                    }
                    .buttonStyle(GhostButton())
                } else {
                    Button {
                        Task { await model.connectSelected() }
                    } label: {
                        Text("Connect")
                            .font(JC.Fonts.caption)
                    }
                    .buttonStyle(GhostButton())
                    .disabled(model.selectedServer == nil || model.connectionState == .connecting)
                }
            }
            .glassCard()

            if let error = model.errorMessage {
                HStack(spacing: JC.Spacing.sm) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.caption)
                    Text(error)
                        .font(JC.Fonts.caption)
                }
                .foregroundStyle(JC.Colors.destructive)
            }
        }
    }

    private var serversSection: some View {
        VStack(alignment: .leading, spacing: JC.Spacing.md) {
            HStack {
                SectionHeader(title: "Servers")
                Spacer()
                HStack(spacing: JC.Spacing.sm) {
                    Button {
                        showQRScanner = true
                    } label: {
                        Image(systemName: "qrcode.viewfinder")
                            .font(.system(size: 16))
                            .foregroundStyle(JC.Colors.accent)
                    }
                    Button {
                        showAddServer = true
                    } label: {
                        Image(systemName: "plus.circle.fill")
                            .font(.system(size: 16))
                            .foregroundStyle(JC.Colors.accent)
                    }
                }
            }

            if model.savedServers.isEmpty {
                VStack(spacing: JC.Spacing.sm) {
                    Image(systemName: "server.rack")
                        .font(.system(size: 24))
                        .foregroundStyle(JC.Colors.textTertiary)
                    Text("No paired servers")
                        .font(JC.Fonts.callout)
                        .foregroundStyle(JC.Colors.textSecondary)
                }
                .frame(maxWidth: .infinity)
                .glassCard()
            } else {
                VStack(spacing: JC.Spacing.sm) {
                    ForEach(model.savedServers, id: \.self) { credential in
                        ServerCard(
                            credential: credential,
                            isSelected: model.selectedServer?.host == credential.host && model.selectedServer?.port == credential.port
                        )
                    }
                }
            }
        }
    }

    private var sessionsSection: some View {
        VStack(alignment: .leading, spacing: JC.Spacing.md) {
            SectionHeader(title: "Sessions")

            if model.connectionState != .connected {
                Text("Connect to view sessions")
                    .font(JC.Fonts.callout)
                    .foregroundStyle(JC.Colors.textTertiary)
                    .frame(maxWidth: .infinity)
                    .glassCard()
            } else if model.sessions.isEmpty {
                Text("No active sessions")
                    .font(JC.Fonts.callout)
                    .foregroundStyle(JC.Colors.textTertiary)
                    .frame(maxWidth: .infinity)
                    .glassCard()
            } else {
                VStack(spacing: JC.Spacing.xs) {
                    ForEach(model.sessions, id: \.self) { sessionId in
                        Button {
                            Task { await model.switchToSession(sessionId) }
                        } label: {
                            HStack(spacing: JC.Spacing.sm) {
                                Image(systemName: "terminal")
                                    .font(.system(size: 12))
                                    .foregroundStyle(JC.Colors.textTertiary)
                                    .frame(width: 20)

                                Text(sessionId)
                                    .font(JC.Fonts.mono)
                                    .foregroundStyle(JC.Colors.textPrimary)
                                    .lineLimit(1)
                                    .truncationMode(.middle)

                                Spacer()

                                if sessionId == model.activeSessionId {
                                    Image(systemName: "checkmark.circle.fill")
                                        .font(.system(size: 14))
                                        .foregroundStyle(JC.Colors.accent)
                                }
                            }
                            .padding(.horizontal, JC.Spacing.md)
                            .padding(.vertical, JC.Spacing.sm + 2)
                            .background(
                                sessionId == model.activeSessionId
                                    ? JC.Colors.accentDim
                                    : JC.Colors.surface
                            )
                            .clipShape(RoundedRectangle(cornerRadius: JC.Radius.sm, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: JC.Radius.sm, style: .continuous)
                                    .stroke(
                                        sessionId == model.activeSessionId
                                            ? JC.Colors.borderFocused
                                            : JC.Colors.border,
                                        lineWidth: 1
                                    )
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    private var modelSection: some View {
        Group {
            if model.connectionState == .connected && !model.availableModels.isEmpty {
                VStack(alignment: .leading, spacing: JC.Spacing.md) {
                    SectionHeader(title: "Model")

                    VStack(spacing: JC.Spacing.xs) {
                        ForEach(model.availableModels, id: \.self) { m in
                            Button {
                                Task { await model.changeModel(m) }
                            } label: {
                                HStack(spacing: JC.Spacing.sm) {
                                    Image(systemName: "cpu")
                                        .font(.system(size: 12))
                                        .foregroundStyle(JC.Colors.textTertiary)
                                        .frame(width: 20)

                                    Text(m)
                                        .font(JC.Fonts.mono)
                                        .foregroundStyle(JC.Colors.textPrimary)
                                        .lineLimit(1)
                                        .truncationMode(.middle)

                                    Spacer()

                                    if m == model.modelName {
                                        Image(systemName: "checkmark.circle.fill")
                                            .font(.system(size: 14))
                                            .foregroundStyle(JC.Colors.accent)
                                    }
                                }
                                .padding(.horizontal, JC.Spacing.md)
                                .padding(.vertical, JC.Spacing.sm + 2)
                                .background(
                                    m == model.modelName
                                        ? JC.Colors.accentDim
                                        : JC.Colors.surface
                                )
                                .clipShape(RoundedRectangle(cornerRadius: JC.Radius.sm, style: .continuous))
                                .overlay(
                                    RoundedRectangle(cornerRadius: JC.Radius.sm, style: .continuous)
                                        .stroke(
                                            m == model.modelName
                                                ? JC.Colors.borderFocused
                                                : JC.Colors.border,
                                            lineWidth: 1
                                        )
                                )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
        }
    }

    private var statusColor: Color {
        switch model.connectionState {
        case .connected: JC.Colors.statusOnline
        case .connecting: JC.Colors.statusConnecting
        case .disconnected: JC.Colors.statusOffline
        }
    }

    private var statusText: String {
        switch model.connectionState {
        case .connected: "Connected"
        case .connecting: "Connecting..."
        case .disconnected: "Disconnected"
        }
    }
}

// MARK: - Section Header

struct SectionHeader: View {
    let title: String

    var body: some View {
        Text(title.uppercased())
            .font(JC.Fonts.caption)
            .foregroundStyle(JC.Colors.textTertiary)
            .tracking(1.2)
    }
}

// MARK: - Server Card

struct ServerCard: View {
    @EnvironmentObject private var model: AppModel
    let credential: ServerCredential
    let isSelected: Bool

    var body: some View {
        Button {
            model.selectedServer = credential
            model.hostInput = credential.host
            model.portInput = String(credential.port)
        } label: {
            HStack(spacing: JC.Spacing.md) {
                ZStack {
                    RoundedRectangle(cornerRadius: JC.Radius.sm, style: .continuous)
                        .fill(isSelected ? JC.Colors.accentDim : JC.Colors.surfaceElevated)
                        .frame(width: 40, height: 40)
                    Image(systemName: "server.rack")
                        .font(.system(size: 16))
                        .foregroundStyle(isSelected ? JC.Colors.accent : JC.Colors.textTertiary)
                }

                VStack(alignment: .leading, spacing: 2) {
                    Text(credential.serverName)
                        .font(JC.Fonts.headline)
                        .foregroundStyle(JC.Colors.textPrimary)
                    HStack(spacing: JC.Spacing.xs) {
                        Text("\(credential.host):\(credential.port)")
                            .font(JC.Fonts.monoSmall)
                            .foregroundStyle(JC.Colors.textTertiary)
                        Text(credential.serverVersion)
                            .font(JC.Fonts.monoCaption)
                            .foregroundStyle(JC.Colors.textTertiary)
                    }
                }

                Spacer()

                if isSelected {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundStyle(JC.Colors.accent)
                }

                Button(role: .destructive) {
                    Task { await model.deleteServer(credential) }
                } label: {
                    Image(systemName: "trash")
                        .font(.system(size: 14))
                        .foregroundStyle(JC.Colors.destructive.opacity(0.6))
                }
                .buttonStyle(.plain)
            }
            .padding(JC.Spacing.md)
            .background(isSelected ? JC.Colors.accentDim.opacity(0.3) : JC.Colors.surface)
            .clipShape(RoundedRectangle(cornerRadius: JC.Radius.md, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: JC.Radius.md, style: .continuous)
                    .stroke(isSelected ? JC.Colors.borderFocused : JC.Colors.border, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Add Server Sheet

struct AddServerSheet: View {
    @EnvironmentObject private var model: AppModel
    @Binding var isPresented: Bool

    var body: some View {
        NavigationStack {
            ZStack {
                JC.Colors.background.ignoresSafeArea()

                ScrollView {
                    VStack(spacing: JC.Spacing.xl) {
                        VStack(spacing: JC.Spacing.sm) {
                            Text("Add Server")
                                .font(JC.Fonts.title2)
                                .foregroundStyle(JC.Colors.textPrimary)

                            Text("Run `jcode pair` on your machine\nto get a pairing code.")
                                .font(JC.Fonts.callout)
                                .foregroundStyle(JC.Colors.textSecondary)
                                .multilineTextAlignment(.center)
                        }

                        VStack(spacing: JC.Spacing.md) {
                            JCTextField(
                                label: "Host",
                                placeholder: "e.g. my-macbook",
                                text: $model.hostInput,
                                icon: "server.rack"
                            )
                            JCTextField(
                                label: "Port",
                                placeholder: "7643",
                                text: $model.portInput,
                                icon: "number",
                                keyboardType: .numberPad
                            )
                            JCTextField(
                                label: "Pair Code",
                                placeholder: "6-digit code",
                                text: $model.pairCodeInput,
                                icon: "key.fill"
                            )
                        }

                        if let error = model.errorMessage {
                            HStack(spacing: JC.Spacing.sm) {
                                Image(systemName: "exclamationmark.triangle.fill")
                                    .font(.caption)
                                Text(error)
                                    .font(JC.Fonts.caption)
                            }
                            .foregroundStyle(JC.Colors.destructive)
                        }

                        Button {
                            Task {
                                await model.pairAndSave()
                                if model.errorMessage == nil {
                                    isPresented = false
                                }
                            }
                        } label: {
                            HStack {
                                Image(systemName: "link")
                                Text("Pair & Connect")
                            }
                            .frame(maxWidth: .infinity)
                        }
                        .buttonStyle(AccentButton())
                    }
                    .padding(JC.Spacing.xl)
                }
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { isPresented = false }
                        .foregroundStyle(JC.Colors.textSecondary)
                }
            }
        }
        .presentationDetents([.medium])
        .presentationBackground(JC.Colors.background)
    }
}
