on run
	set repo to "/Users/gim-yeonsu/dominion-rust"
	do shell script "cd " & quoted form of repo & " && /usr/bin/env zsh ./start-rust.command >/dev/null 2>&1 &"
end run
