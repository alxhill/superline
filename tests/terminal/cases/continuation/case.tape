Wait+Screen /\x{F105}/
Type "echo 'one"
Enter
Sleep 1s
Type "two'"
Enter
Wait+Screen /\ntwo\n(?s:.*)\x{F105}/
Screenshot after.png
