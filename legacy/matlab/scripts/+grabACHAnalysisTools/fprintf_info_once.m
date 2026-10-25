function fprintf_info_once(messageID, varargin)
% FPRINTF_INFO_ONCE Prints a message to the console only once per messageID.
%   Useful for reducing repetitive warnings or info messages in loops.
%
% Args:
%   messageID (string/char): A unique identifier for this message type.
%   varargin: Arguments to pass to fprintf (formatSpec, A1, ..., An).

    persistent reportedMessages; % Stays in memory between calls

    if isempty(reportedMessages)
        reportedMessages = containers.Map('KeyType', 'char', 'ValueType', 'logical');
    end

    messageID_char = char(messageID); % Ensure char for map key

    if ~isKey(reportedMessages, messageID_char)
        fprintf(varargin{:});
        reportedMessages(messageID_char) = true; % Mark as reported
    end
end