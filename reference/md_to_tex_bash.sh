#!/usr/bin/env bash

processLine() {
	echo "$1" | sed -E \
		-e "s|&|\\\&|g" \
		-e "s|\^(.*)\^|\\\textsuperscript{\1}|g" \
		-e "s|\*(.*)\*|\\\textbf{\1}|g" \
		-e "s|\`(.*)\`|\\\texttt{\1}|g" \
		-e "s|'(.*)'|\`\1'|g" \
		-e "s|\"(.*)\"|\`\`\1''|g" \
		-e "s| _(.*)_ | \\\emph{\1} |g"
}

processHeading() {
	heading="$1"
	text="$2"
	ref="$(echo $text | sed -E 's|\[]\(#(.*)\).*|\1|' )"
	title="$(echo $text | sed -E 's|\[]\(#.*\)(.*)|\1|')"
	echo "\\${heading}{$(processLine "${title}")}\\label{${ref}}"
}

while IFS='' read -r line; do
	if [[ "$line" == "# "* ]]; then
		printf "" # Ignore the line
	else
		processLine "$line"
	fi
done < "../abstract.md" > "abstract.tex"

STATE="TEXT"

while IFS='' read -r line; do

	case "$STATE" in
		ORDERED)
			if [[ -z "$line" ]]; then
				# Completed the list, return to regular text
				STATE="TEXT"
				echo "\\end{enumerate}"
				echo
			else
				echo "\\item $(processLine "$(echo "$line" | sed -E 's|^([0-9])+. ||g')")"
			fi
		;;
		UNORDERED)
			if [[ -z "$line" ]]; then
				# Completed the list, return to regular text
				STATE="TEXT"
				echo "\\end{itemize}"
				echo
			else
				echo "\\item $(processLine "${line:2}")"
			fi
		;;
		QUOTE)
			# \usepackage{csquotes}
			if [[ -z "$line" ]]; then
				# Completed the block quote, return to regular text
				STATE="TEXT"
				echo "\\end{displayquote}"
			else
				processLine "$line"
			fi
		;;
		CODE)
			# \usepackage{listings}
			# \lstset{...} to configure the language"
			if [[ "$line" == '```' ]]; then
				# Completed the code block, return to regular text
				STATE="TEXT"
				echo "\\end{lstlisting}"
			else
				# No additional processing of the text
				echo "$line"
			fi
		;;
		FIGURE)
			if [[ -z "$line" ]]; then
				# Completed the table, start on the caption
				STATE="FIG_CAPTION"
				printf "\\\caption{"
			else
				# No additional processing of the text
				echo "$line"
			fi
		;;
		TABLE)
			# \usepackage{booktabs}
			if [[ -z "$line" ]]; then
				# Completed the table, start on the caption
				STATE="TBL_CAPTION"
				echo "\\bottomrule"
				echo "\\end{tabular}"
				printf "\\\caption{"
			elif [[ "$line" == "|<!--"* ]]; then
				# Configuration for this table; must come after the header and divider, i.e.,
				# | Table | Heading |
				# |-------|---------|
				# <!--line header only-->
				if [[ "$line" == *"line every row"* ]]; then
					# Put a line between each row
					LINE_EVERY_ROW=true
				elif [[ "$line" == *"line header only"* ]]; then
					# Put a line under the heading only
					echo "\\midrule"
				fi
			elif [[ "$line" == "|---"* || "$line" == "| ---"* ]]; then
				# The header line
				printf "" # Ignore the line
			else
				# A table row
				if $LINE_EVERY_ROW; then echo "\\midrule"; fi
				readarray -td "|" a <<< "$line"; unset 'a[0]'; unset 'a[${#a[@]}]';
				IS_FIRST=true
				for body in "${a[@]}"; do
					if $IS_FIRST; then IS_FIRST=false;
					else printf " & "; fi
					printf "%s" "$(processLine "$(echo "$body" | xargs)")"
				done
				echo " \\\\"
			fi
		;;
		FIG_CAPTION)
			if [[ -z "$line" ]]; then
				# Completed the caption, return to regular text
				STATE="TEXT"
				echo "}"
				echo "\\end{figure}"
				echo
			else
				processLine "$line"
			fi
		;;
		TBL_CAPTION)
			if [[ -z "$line" ]]; then
				# Completed the caption, return to regular text
				STATE="TEXT"
				echo "}"
				echo "\\end{table}"
				echo
			else
				processLine "$line"
			fi
		;;
		TEXT)
			if [[ "$line" == "##### "* ]]; then
				# This is a sub-subsection
				text="${line#"#####" *}"
				processHeading "subsubsection" "$text"
			elif [[ "$line" == "#### "* ]]; then
				# This is a subsection
				text="${line#"####" *}"
				processHeading "subsection" "$text"
			elif [[ "$line" == "### "* ]]; then
				# This is a section
				text="${line#"###" *}"
				processHeading "section" "$text"
			elif [[ "$line" == "## "* ]]; then
				# This is a chapter
				text="${line#"##" *}"
				processHeading "chapter" "$text"
			elif [[ "$line" == "# "* ]]; then
				# This is a comment (technically a title in md, but we can't do anything with it here)
				printf "" # Ignore the line
			elif [[ "$line" == "|figure" ]]; then
				# We're starting a figure
				STATE="FIGURE"
				echo "\\begin{figure}"
			elif [[ "$line" == "|"* ]]; then
				# We're starting a table.  Output the header and change state.
				STATE="TABLE"
				LINE_EVERY_ROW=false
				readarray -td "|" a <<< "$line"; unset 'a[0]'; unset 'a[${#a[@]}]';
				echo "\\begin{table}"
				printf "\\\begin{tabular}{"
				for heading in "${a[@]}"; do
					COL_DESC="$(echo "$heading" | sed -n -E 's|.*<!--(.*)-->.*|\1|p')"
					if [[ -z "$COL_DESC" ]]; then COL_DESC=l; fi
					printf " ${COL_DESC} "
				done
				echo "}"
				echo "\\toprule"
				IS_FIRST=true
				for heading in "${a[@]}"; do
					heading="$(echo "$heading" | sed -E 's|<!--.*-->||g')"
					if $IS_FIRST; then IS_FIRST=false;
					else printf " & "; fi
					printf "\\\textbf{%s}" "$(echo "$heading" | xargs)"
				done
				echo " \\\\"
			elif [[ "$line" == '```'* ]]; then
				STATE="CODE"
				CODE_LANG="${line#'```'*}"
				printf "\\\\begin{lstlisting}"
				if [[ -n "$CODE_LANG" ]]; then printf "%s" "[language=$CODE_LANG]"; fi
				echo
			elif [[ "$line" == "> "* ]]; then
				STATE="QUOTE"
				echo "\\begin{displayquote}"
				processLine "${line#> *}"
			elif [[ "$line" == '* '* || "$line" == '- '* || "$line" == '+ '* ]]; then
				STATE="UNORDERED"
				echo "\\begin{itemize}"
				echo "\\item $(processLine "${line:2}")"
			elif [[ "$line" =~ ^([0-9])+". "* ]]; then
				STATE="ORDERED"
				echo "\\begin{enumerate}"
				echo "\\item $(processLine "$(echo "$line" | sed -E 's|^([0-9])+. ||g')")"
			elif [[ "$line" == '['*']('*')'* ]]; then
				# A URL.  Only use the textual part, even though the other contains the actual
				# link destination.
				echo "$line" | sed -E -e "s|\[(.*)\]\(.*\)|\\\url{\1}|g"
			elif [[ "$line" == '`'*'`'* ]]; then
				# A line that looks like the following
				# `variable['yada']`,
				# where we don't want the single quotes being handled, but we
				# do want to keep the trailing punctuation.
				echo "$line" | sed -E -e "s|\`(.*)\`|\\\texttt{\1}|g"
			else
				# Regular text
				processLine "$line"
			fi
		;;
		*)
			echo "Error, unknown state $STATE" 2>1
			exit 1
		;;
	esac
done < "../content.md" > "content.tex"

echo "Tex files built.  Now run"
echo "xelatex paper.tex && bibtex paper && xelatex paper.tex && xelatex paper.tex"
