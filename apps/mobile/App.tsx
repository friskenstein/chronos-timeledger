import { StatusBar } from 'expo-status-bar'
import { SafeAreaView, ScrollView, StyleSheet, Text, View } from 'react-native'

const bullets = [
	'Reads and writes the shared .ledger datastore.',
	'Targets the same TOML header and JSONL event log as the Rust TUI.',
	'Keeps the file contract documented under contracts/spec/format-v1.md.',
]

export default function App() {
	return (
		<SafeAreaView style={styles.safeArea}>
			<StatusBar style="dark" />
			<ScrollView contentContainerStyle={styles.scrollContent}>
				<View style={styles.card}>
					<Text style={styles.eyebrow}>Chronos</Text>
					<Text style={styles.title}>Mobile workspace scaffold</Text>
					<Text style={styles.copy}>
						This app is set up as the future mobile client for the shared ledger
						format already used by the TUI.
					</Text>
				</View>
				<View style={styles.card}>
					<Text style={styles.sectionTitle}>Contract targets</Text>
					{bullets.map((bullet) => (
						<Text key={bullet} style={styles.bullet}>
							{`\u2022 ${bullet}`}
						</Text>
					))}
				</View>
			</ScrollView>
		</SafeAreaView>
	)
}

const styles = StyleSheet.create({
	safeArea: {
		flex: 1,
		backgroundColor: '#f4efe7',
	},
	scrollContent: {
		flexGrow: 1,
		paddingHorizontal: 24,
		paddingVertical: 32,
		gap: 20,
	},
	card: {
		backgroundColor: '#fffaf2',
		borderRadius: 24,
		padding: 24,
		borderWidth: 1,
		borderColor: '#d6c4ab',
		shadowColor: '#7a5c35',
		shadowOpacity: 0.08,
		shadowRadius: 18,
		shadowOffset: {
			width: 0,
			height: 12,
		},
		elevation: 3,
	},
	eyebrow: {
		fontSize: 13,
		letterSpacing: 2.2,
		textTransform: 'uppercase',
		color: '#8a5a24',
		marginBottom: 12,
	},
	title: {
		fontSize: 32,
		lineHeight: 38,
		fontWeight: '700',
		color: '#20150a',
		marginBottom: 12,
	},
	copy: {
		fontSize: 16,
		lineHeight: 24,
		color: '#4c3a28',
	},
	sectionTitle: {
		fontSize: 18,
		fontWeight: '600',
		color: '#20150a',
		marginBottom: 12,
	},
	bullet: {
		fontSize: 15,
		lineHeight: 22,
		color: '#4c3a28',
		marginBottom: 10,
	},
})
