function [epochs_signal, epoch_time_s_relative, valid_event_indices] = ...
    extract_peri_event_epochs(time_s, signal_vector, event_times_s, ...
                              pre_window_sec, post_window_sec, fs)
% EXTRACT_PERI_EVENT_EPOCHS Extracts peri-event epochs from a signal vector.
%
% Args:
%   time_s (vector): Full time vector of the recording in seconds. Column vector.
%   signal_vector (vector): Full signal vector. Column vector.
%   event_times_s (vector): Times of event onsets in seconds.
%   pre_window_sec (scalar): Duration before event onset (s). Non-negative.
%   post_window_sec (scalar): Duration after event onset (s). Non-negative.
%   fs (scalar): Sampling rate (Hz). Positive.
%
% Returns:
%   epochs_signal (matrix): (num_samples_per_epoch x num_valid_epochs).
%   epoch_time_s_relative (vector): Time vector for epochs, relative to event (0s).
%   valid_event_indices (vector): Row vector of original event indices used.

    arguments
        time_s (:,1) double {mustBeNumeric, mustBeReal}
        signal_vector (:,1) double {mustBeNumeric, mustBeReal}
        event_times_s (:,:) double {mustBeNumeric, mustBeReal}
        pre_window_sec (1,1) double {mustBeNumeric, mustBeReal, mustBeNonnegative}
        post_window_sec (1,1) double {mustBeNumeric, mustBeReal, mustBeNonnegative}
        fs (1,1) double {mustBeNumeric, mustBeReal, mustBePositive}
    end

    epochs_signal = [];
    epoch_time_s_relative = [];
    valid_event_indices = [];

    if isempty(event_times_s), warning('grabACHAnalysisTools:extract_peri_event_epochs:NoEventTimes', 'No event times. Returning empty.'); return; end
    if isempty(time_s) || isempty(signal_vector), warning('grabACHAnalysisTools:extract_peri_event_epochs:EmptySignal', 'Empty time or signal. Returning empty.'); return; end
    if length(time_s) ~= length(signal_vector), error('grabACHAnalysisTools:extract_peri_event_epochs:LengthMismatch', 'Time and signal vectors length mismatch.'); end
    
    event_times_s = event_times_s(:);

    pre_samples = round(pre_window_sec * fs);
    post_samples = round(post_window_sec * fs);
    total_epoch_samples = pre_samples + post_samples + 1;

    if total_epoch_samples <= 0, warning('grabACHAnalysisTools:extract_peri_event_epochs:InvalidEpochDuration', 'Total epoch samples <= 0. Returning empty.'); return; end
    
    epoch_time_s_relative = linspace(-pre_window_sec, post_window_sec, total_epoch_samples)';
    num_events = length(event_times_s);
    epochs_signal_list = cell(1, num_events);
    valid_event_flags = false(1, num_events);

    for i = 1:num_events
        current_event_time_s = event_times_s(i);
        [~, event_idx_in_full_signal] = min(abs(time_s - current_event_time_s));
        start_idx = event_idx_in_full_signal - pre_samples;
        end_idx = event_idx_in_full_signal + post_samples;
        
        if start_idx >= 1 && end_idx <= length(signal_vector)
            epochs_signal_list{i} = signal_vector(start_idx:end_idx);
            valid_event_flags(i) = true;
        else
            fprintf('Info: Event %d (original index, time: %.2fs) too close to boundaries. Skipping.\n', i, current_event_time_s);
        end
    end

    valid_event_indices = find(valid_event_flags);
    if isempty(valid_event_indices), warning('grabACHAnalysisTools:extract_peri_event_epochs:NoValidEpochs', 'No valid epochs extracted from %d events.', num_events); epochs_signal = []; return; end
    
    epochs_signal = horzcat(epochs_signal_list{valid_event_flags});
    if size(epochs_signal,1) ~= total_epoch_samples && size(epochs_signal,2) == total_epoch_samples && total_epoch_samples > 0
        epochs_signal = epochs_signal';
    end
    if ~isempty(epochs_signal) && size(epochs_signal,1) ~= total_epoch_samples
         error('grabACHAnalysisTools:extract_peri_event_epochs:DimensionError', 'Final epoch matrix dimensions incorrect.');
    end
end