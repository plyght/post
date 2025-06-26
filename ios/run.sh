#!/bin/bash

echo "🚀 Building and running Post iOS app..."

# Build the project
echo "Building..."
xcodebuild -project PostiOS.xcodeproj \
           -scheme PostiOS \
           -destination "platform=iOS Simulator,id=544BDFBC-0BAE-44FA-9F3B-8141B708FE01" \
           build

if [ $? -eq 0 ]; then
    echo "✅ Build successful!"
    echo ""
    echo "🎉 Post iOS app is ready!"
    echo "📱 Open Xcode and run the app to see it in action:"
    echo "   open PostiOS.xcodeproj"
    echo ""
    echo "📋 Features implemented:"
    echo "   • SwiftUI interface with 3 tabs (Status, Peers, Settings)"
    echo "   • iOS 26 beta compatibility"
    echo "   • Preview Content structure"
    echo "   • App Icons and Accent Colors configured"
    echo ""
    echo "🔧 Next steps:"
    echo "   • Connect to your existing post daemon"
    echo "   • Implement PostCommon framework integration"
    echo "   • Add App Intents for Shortcuts"
    echo "   • Create share sheet extension"
else
    echo "❌ Build failed!"
    exit 1
fi