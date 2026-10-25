% Number of columns to skip
skip_columns = 1000; % Adjust based on initial fluorescence artifact

% Skip initial artifact period
datagreen_skipped = datagreen(skip_columns + 1:end);
datablue_skipped = datablue(skip_columns + 1:end);
ts_skipped = ts(skip_columns + 1:end);

% Calculate baseline fluorescence (F0) using second-order least mean
% squares regression for 405 channel
window_size = 3000; % Adjust based on recording conditions
F0_blue = movmean(datablue_skipped, window_size);

%% Calculate baseline fluorescence (F0) using second-order least mean
% squares regression for 465 channel
F0_green = movmean(datagreen_skipped, window_size); 

% Ensure F0 vectors match the length of skipped data
F0_green = F0_green(1:length(datagreen_skipped));
F0_blue = F0_blue(1:length(datablue_skipped));

% Normalize datagreen_skipped by F0_blue (reference channel)
dfof_green = (datagreen_skipped ./ F0_blue) * 100; % Directly normalize by F0_blue
dfof_blue = (datablue_skipped ./ F0_blue) * 100;   % Normalize blue by F0_blue

% Plot normalized data (ΔF/F0 for both channels)
figure;
plot(ts_skipped, dfof_green, 'b-');
hold on;
plot(ts_skipped, dfof_blue, 'r-');
title('Normalized \DeltaF/F0 for Signal (465nm) and Reference (405nm)');
legend('465nm (\DeltaF/F0)', '405nm (\DeltaF/F0)');
hold off;

% Moving average smoothing of ΔF/F0 (green channel)
smoothed_dfof_green = smooth(dfof_green, 60, 'moving');

% Plot smoothed ΔF/F0
figure;
plot(ts_skipped, smoothed_dfof_green, 'b-');
%ylim([-2, 10]);
%xlim([2, 100]);
title('Smoothed \DeltaF/F0 for Green Channel');

% Calculate Z-score from averaged ΔF/F0 (green channel)
zscore_green = zscore(smoothed_dfof_green);

% Plot Z-score
figure;
plot(ts_skipped, zscore_green);
ylim([-3, 5]);
xlim([0, 700]);
title('Z-score of \DeltaF/F0 (Green Channel)');

% Transpose time vector for export
ts_skipped_transposed = transpose(ts_skipped);

% Create table and export as CSV
T = table(ts_skipped_transposed, zscore_green);
filename = 'exported_data.csv';
writetable(T, filename);

% Display confirmation message
disp('Data has been exported to exported_data.csv');